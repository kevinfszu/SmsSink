//! Win32 原语：手写 FFI，保持二进制小、依赖少。
//! 仅 Windows 目标可用；非 Windows 下本模块整体不参与编译。
#![allow(non_snake_case, non_camel_case_types, dead_code)]

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::time::{SystemTime, UNIX_EPOCH};

pub type HWND = isize;
pub type HINSTANCE = isize;
pub type HMENU = isize;
pub type HICON = isize;
pub type HANDLE = isize;
pub type HBRUSH = isize;
pub type HFONT = isize;
pub type HDC = isize;
pub type HGDIOBJ = isize;
pub type LPARAM = isize;
pub type WPARAM = usize;
pub type LRESULT = isize;
pub type UINT = u32;
pub type DWORD = u32;
pub type BOOL = i32;
pub type ATOM = u16;
pub type COLORREF = u32;
pub type WNDPROC = Option<unsafe extern "system" fn(HWND, UINT, WPARAM, LPARAM) -> LRESULT>;

#[repr(C)]
pub struct WNDCLASSW {
    pub style: UINT,
    pub lpfnWndProc: WNDPROC,
    pub cbClsExtra: i32,
    pub cbWndExtra: i32,
    pub hInstance: HINSTANCE,
    pub hIcon: HICON,
    pub hCursor: HANDLE,
    pub hbrBackground: HBRUSH,
    pub lpszMenuName: *const u16,
    pub lpszClassName: *const u16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: UINT,
    pub wParam: WPARAM,
    pub lParam: LPARAM,
    pub time: DWORD,
    pub pt: POINT,
    pub lPrivate: DWORD,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct LOGFONTW {
    pub lfHeight: i32,
    pub lfWidth: i32,
    pub lfEscapement: i32,
    pub lfOrientation: i32,
    pub lfWeight: i32,
    pub lfItalic: u8,
    pub lfUnderline: u8,
    pub lfStrikeOut: u8,
    pub lfCharSet: u8,
    pub lfOutPrecision: u8,
    pub lfClipPrecision: u8,
    pub lfQuality: u8,
    pub lfPitchAndFamily: u8,
    pub lfFaceName: [u16; 32],
}

pub type FONTENUMPROCW =
    Option<unsafe extern "system" fn(*const LOGFONTW, *const c_void, DWORD, LPARAM) -> i32>;

/// 自绘按钮的绘制上下文（WM_DRAWITEM 的 lParam）
#[repr(C)]
pub struct DRAWITEMSTRUCT {
    pub CtlType: UINT,
    pub CtlID: UINT,
    pub itemID: UINT,
    pub itemAction: UINT,
    pub itemState: UINT,
    pub hwndItem: HWND,
    pub hDC: HDC,
    pub rcItem: RECT,
    pub itemData: usize,
}

/// 鼠标离开窗口的订阅（WM_MOUSELEAVE 只有订阅后才会收到）
#[repr(C)]
pub struct TRACKMOUSEEVENT {
    pub cbSize: DWORD,
    pub dwFlags: DWORD,
    pub hwndTrack: HWND,
    pub dwHoverTime: DWORD,
}

pub const TME_LEAVE: DWORD = 0x0000_0002;
pub const WM_MOUSEMOVE: UINT = 0x0200;
pub const WM_MOUSELEAVE: UINT = 0x02A3;

/// 窗口圆角（Win11）
pub const DWMWA_WINDOW_CORNER_PREFERENCE: DWORD = 33;
pub const DWMWCP_ROUND: i32 = 2;

#[repr(C)]
pub struct PAINTSTRUCT {
    pub hdc: HDC,
    pub fErase: BOOL,
    pub rcPaint: RECT,
    pub fRestore: BOOL,
    pub fIncUpdate: BOOL,
    pub rgbReserved: [u8; 32],
}

#[repr(C)]
#[derive(Default)]
pub struct SYSTEMTIME {
    pub wYear: u16,
    pub wMonth: u16,
    pub wDayOfWeek: u16,
    pub wDay: u16,
    pub wHour: u16,
    pub wMinute: u16,
    pub wSecond: u16,
    pub wMilliseconds: u16,
}

#[repr(C)]
pub struct NOTIFYICONDATAW {
    pub cbSize: DWORD,
    pub hWnd: HWND,
    pub uID: UINT,
    pub uFlags: UINT,
    pub uCallbackMessage: UINT,
    pub hIcon: HICON,
    pub szTip: [u16; 128],
    pub dwState: DWORD,
    pub dwStateMask: DWORD,
    pub szInfo: [u16; 256],
    pub uVersion: UINT,
    pub szInfoTitle: [u16; 64],
    pub dwInfoFlags: DWORD,
    pub guidItem: [u8; 16],
    pub hBalloonIcon: HICON,
}

pub const WM_APP: UINT = 0x8000;
pub const WM_DESTROY: UINT = 0x0002;
pub const WM_PAINT: UINT = 0x000F;
pub const WM_CLOSE: UINT = 0x0010;
pub const WM_COMMAND: UINT = 0x0111;
pub const WM_DRAWITEM: UINT = 0x002B;
pub const WM_TIMER: UINT = 0x0113;
pub const WM_RBUTTONUP: UINT = 0x0205;
pub const WM_LBUTTONUP: UINT = 0x0202;
pub const WM_LBUTTONDBLCLK: UINT = 0x0203;
pub const WM_CTLCOLORSTATIC: UINT = 0x0138;

pub const WS_CHILD: DWORD = 0x4000_0000;
pub const WS_VISIBLE: DWORD = 0x1000_0000;
pub const WS_POPUP: DWORD = 0x8000_0000;
pub const WS_TABSTOP: DWORD = 0x0001_0000;
pub const BS_OWNERDRAW: DWORD = 0x0000_000B;
pub const ODS_SELECTED: UINT = 0x0001;
pub const ODS_DISABLED: UINT = 0x0004;
pub const WS_EX_TOOLWINDOW: DWORD = 0x0000_0080;
pub const WS_EX_TOPMOST: DWORD = 0x0000_0008;
pub const WS_EX_NOACTIVATE: DWORD = 0x0800_0000;
pub const WS_EX_LAYERED: DWORD = 0x0008_0000;

pub const HWND_MESSAGE: HWND = -3;
pub const HWND_TOPMOST: HWND = -1;
pub const SWP_NOACTIVATE: UINT = 0x0010;
pub const SW_SHOWNOACTIVATE: i32 = 4;
pub const GWLP_USERDATA: i32 = -21;

pub const NIM_ADD: DWORD = 0;
pub const NIM_DELETE: DWORD = 2;
pub const NIM_SETVERSION: DWORD = 4;
pub const NIF_MESSAGE: UINT = 0x01;
pub const NIF_ICON: UINT = 0x02;
pub const NIF_TIP: UINT = 0x04;
/// 气泡通知（托盘图标上弹出的提示条）
pub const NIF_INFO: UINT = 0x10;
pub const NIIF_INFO: DWORD = 0x1;
pub const NIIF_WARNING: DWORD = 0x2;
pub const NIIF_ERROR: DWORD = 0x3;
/// 气泡不发声，避免打断正在进行的输入
pub const NIIF_NOSOUND: DWORD = 0x10;
pub const NOTIFYICON_VERSION_4: UINT = 4;

pub const MB_OK: UINT = 0x0;
pub const MB_ICONERROR: UINT = 0x10;
pub const MB_ICONINFORMATION: UINT = 0x40;
pub const MB_SETFOREGROUND: UINT = 0x1_0000;
pub const MB_TOPMOST: UINT = 0x4_0000;

pub const MF_STRING: UINT = 0x0000;
pub const MF_CHECKED: UINT = 0x0008;
pub const MF_UNCHECKED: UINT = 0x0000;
pub const MF_SEPARATOR: UINT = 0x0800;
pub const MF_GRAYED: UINT = 0x0001;
pub const MF_DISABLED: UINT = 0x0002;
pub const MF_BYCOMMAND: UINT = 0x0000;
/// 挂一个子菜单（id 位置传子菜单句柄）
pub const MF_POPUP: UINT = 0x0010;
pub const TPM_RIGHTBUTTON: UINT = 0x0002;
pub const TPM_RETURNCMD: UINT = 0x0100;
pub const TPM_NONOTIFY: UINT = 0x0080;
pub const TPM_BOTTOMALIGN: UINT = 0x0020;
pub const TPM_RIGHTALIGN: UINT = 0x0008;

pub const IDI_APPLICATION: usize = 32512;
pub const IDC_ARROW: usize = 32512;
pub const SPI_GETWORKAREA: UINT = 0x0030;
pub const SM_CXSCREEN: i32 = 0;

pub const DT_LEFT: UINT = 0x0000;
pub const DT_CENTER: UINT = 0x0001;
pub const DT_VCENTER: UINT = 0x0004;
pub const DT_SINGLELINE: UINT = 0x0020;
pub const DT_WORD_ELLIPSIS: UINT = 0x0004_0000;
pub const DT_END_ELLIPSIS: UINT = 0x0000_8000;
pub const DT_NOPREFIX: UINT = 0x0000_0800;
pub const TRANSPARENT: i32 = 1;
/// 不透明背景模式：ClearType 在实色背景上才能拿到最优的子像素抗锯齿，
/// 透明模式下 GDI 需要回读目标像素，边缘会发灰、发软。
pub const OPAQUE: i32 = 2;
/// Per-Monitor V2，Win10 1703+ 的 DPI 感知上下文（伪句柄 -4）
pub const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: HANDLE = -4;
pub const FW_NORMAL: i32 = 400;
pub const FW_SEMIBOLD: i32 = 600;
pub const FW_BOLD: i32 = 700;
pub const DEFAULT_CHARSET: u32 = 1;
pub const CLEARTYPE_QUALITY: u32 = 5;
pub const DEFAULT_PITCH: u32 = 0;
pub const FF_DONTCARE: u32 = 0;
pub const DEFAULT_GUI_FONT: i32 = 17;
/// 用当前 DC 的画刷 / 画笔颜色填充与描边，免去反复创建 GDI 对象
pub const DC_BRUSH: i32 = 18;
pub const DC_PEN: i32 = 19;
pub const NULL_BRUSH: i32 = 5;
pub const CF_UNICODETEXT: UINT = 13;
pub const GMEM_MOVEABLE: UINT = 0x0002;
pub const COINIT_APARTMENTTHREADED: DWORD = 0x2;
pub const ERROR_ALREADY_EXISTS: DWORD = 183;
pub const HKEY_CURRENT_USER: isize = 0x8000_0001_u32 as i32 as isize;
pub const KEY_WRITE: u32 = 0x20006;
pub const KEY_QUERY_VALUE: u32 = 0x1;
pub const REG_SZ: DWORD = 1;
pub const REG_DWORD: DWORD = 4;

// 控制台挂载（GUI 子系统程序按需接上父进程的终端）
pub const ATTACH_PARENT_PROCESS: DWORD = 0xFFFF_FFFF;
pub const STD_OUTPUT_HANDLE: DWORD = -11i32 as DWORD;
pub const STD_ERROR_HANDLE: DWORD = -12i32 as DWORD;
pub const GENERIC_READ: DWORD = 0x8000_0000;
pub const GENERIC_WRITE: DWORD = 0x4000_0000;
pub const FILE_SHARE_READ: DWORD = 0x1;
pub const FILE_SHARE_WRITE: DWORD = 0x2;
pub const OPEN_EXISTING: DWORD = 3;
pub const INVALID_HANDLE_VALUE: HANDLE = -1;

// 图标
pub const IMAGE_ICON: UINT = 1;
pub const LR_SHARED: UINT = 0x8000;
pub const SM_CXSMICON: i32 = 49;
pub const SM_CYSMICON: i32 = 50;
pub const WM_SETICON: UINT = 0x0080;
pub const ICON_SMALL: WPARAM = 0;
pub const ICON_BIG: WPARAM = 1;
/// 资源脚本 app.rc 里主图标的 ID
pub const ICON_RESOURCE_ID: usize = 1;

#[link(name = "user32")]
extern "system" {
    pub fn RegisterClassW(lpWndClass: *const WNDCLASSW) -> ATOM;
    pub fn CreateWindowExW(
        dwExStyle: DWORD,
        lpClassName: *const u16,
        lpWindowName: *const u16,
        dwStyle: DWORD,
        x: i32,
        y: i32,
        nWidth: i32,
        nHeight: i32,
        hWndParent: HWND,
        hMenu: HMENU,
        hInstance: HINSTANCE,
        lpParam: *mut c_void,
    ) -> HWND;
    pub fn DefWindowProcW(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
    pub fn GetMessageW(lpMsg: *mut MSG, hwnd: HWND, min: UINT, max: UINT) -> BOOL;
    pub fn TranslateMessage(lpMsg: *const MSG) -> BOOL;
    pub fn DispatchMessageW(lpMsg: *const MSG) -> LRESULT;
    pub fn PostMessageW(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> BOOL;
    pub fn PostQuitMessage(nExitCode: i32);
    pub fn DestroyWindow(hwnd: HWND) -> BOOL;
    pub fn ShowWindow(hwnd: HWND, nCmdShow: i32) -> BOOL;
    pub fn SetWindowPos(
        hwnd: HWND,
        after: HWND,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: UINT,
    ) -> BOOL;
    pub fn SetWindowLongPtrW(hwnd: HWND, index: i32, value: isize) -> isize;
    pub fn GetWindowLongPtrW(hwnd: HWND, index: i32) -> isize;
    pub fn GetClientRect(hwnd: HWND, rect: *mut RECT) -> BOOL;
    pub fn GetParent(hwnd: HWND) -> HWND;
    pub fn FindWindowW(class: *const u16, title: *const u16) -> HWND;
    pub fn BeginPaint(hwnd: HWND, ps: *mut PAINTSTRUCT) -> HDC;
    pub fn EndPaint(hwnd: HWND, ps: *const PAINTSTRUCT) -> BOOL;
    pub fn FillRect(hdc: HDC, rect: *const RECT, brush: HBRUSH) -> i32;
    pub fn DrawTextW(hdc: HDC, text: *const u16, count: i32, rect: *mut RECT, format: UINT) -> i32;
    pub fn LoadIconW(hInstance: HINSTANCE, name: *const u16) -> HICON;
    pub fn LoadImageW(
        hInstance: HINSTANCE,
        name: *const u16,
        kind: UINT,
        cx: i32,
        cy: i32,
        load: UINT,
    ) -> HANDLE;
    pub fn GetSystemMetrics(index: i32) -> i32;
    pub fn SendMessageW(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
    pub fn LoadCursorW(hInstance: HINSTANCE, name: *const u16) -> HANDLE;
    pub fn CreatePopupMenu() -> HMENU;
    pub fn AppendMenuW(menu: HMENU, flags: UINT, id: usize, text: *const u16) -> BOOL;
    pub fn CheckMenuItem(menu: HMENU, id: UINT, flags: UINT) -> DWORD;
    pub fn TrackPopupMenu(
        menu: HMENU,
        flags: UINT,
        x: i32,
        y: i32,
        reserved: i32,
        hwnd: HWND,
        rect: *const RECT,
    ) -> i32;
    pub fn DestroyMenu(menu: HMENU) -> BOOL;
    pub fn GetCursorPos(point: *mut POINT) -> BOOL;
    pub fn SetForegroundWindow(hwnd: HWND) -> BOOL;
    pub fn SetTimer(hwnd: HWND, id: usize, elapse: UINT, callback: *mut c_void) -> usize;
    pub fn KillTimer(hwnd: HWND, id: usize) -> BOOL;
    pub fn SystemParametersInfoW(
        action: UINT,
        param: UINT,
        value: *mut c_void,
        winini: UINT,
    ) -> BOOL;
    pub fn SetProcessDPIAware() -> BOOL;
    pub fn SetProcessDpiAwarenessContext(context: HANDLE) -> BOOL;
    pub fn GetDpiForSystem() -> UINT;
    pub fn OpenClipboard(hwnd: HWND) -> BOOL;
    pub fn EmptyClipboard() -> BOOL;
    pub fn SetClipboardData(format: UINT, mem: HANDLE) -> HANDLE;
    pub fn CloseClipboard() -> BOOL;
    pub fn MessageBoxW(hwnd: HWND, text: *const u16, caption: *const u16, flags: UINT) -> i32;
    pub fn TrackMouseEvent(event: *mut TRACKMOUSEEVENT) -> BOOL;
    pub fn SetCursor(pos: *mut POINT) -> BOOL;
    pub fn InvalidateRect(hwnd: HWND, rect: *const RECT, erase: BOOL) -> BOOL;
    pub fn UpdateWindow(hwnd: HWND) -> BOOL;
}

#[link(name = "gdi32")]
extern "system" {
    pub fn CreateSolidBrush(color: COLORREF) -> HBRUSH;
    pub fn CreateFontW(
        height: i32,
        width: i32,
        escapement: i32,
        orientation: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strikeout: u32,
        charset: u32,
        out_precision: u32,
        clip_precision: u32,
        quality: u32,
        pitch_and_family: u32,
        face: *const u16,
    ) -> HFONT;
    pub fn GetStockObject(index: i32) -> HGDIOBJ;
    pub fn CreateCompatibleDC(hdc: HDC) -> HDC;
    pub fn DeleteDC(hdc: HDC) -> BOOL;
    pub fn EnumFontFamiliesExW(
        hdc: HDC,
        lf: *const LOGFONTW,
        proc_: FONTENUMPROCW,
        param: LPARAM,
        flags: DWORD,
    ) -> i32;
    pub fn SetBkMode(hdc: HDC, mode: i32) -> i32;
    pub fn SetBkColor(hdc: HDC, color: COLORREF) -> COLORREF;
    pub fn SetTextCharacterExtra(hdc: HDC, extra: i32) -> i32;
    pub fn SetTextColor(hdc: HDC, color: COLORREF) -> COLORREF;
    pub fn SelectObject(hdc: HDC, obj: HGDIOBJ) -> HGDIOBJ;
    pub fn DeleteObject(obj: HGDIOBJ) -> BOOL;
    pub fn SetDCBrushColor(hdc: HDC, color: COLORREF) -> COLORREF;
    pub fn SetDCPenColor(hdc: HDC, color: COLORREF) -> COLORREF;
    pub fn RoundRect(
        hdc: HDC,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        width: i32,
        height: i32,
    ) -> BOOL;
}

#[link(name = "dwmapi")]
extern "system" {
    pub fn DwmSetWindowAttribute(
        hwnd: HWND,
        attribute: DWORD,
        value: *const c_void,
        size: DWORD,
    ) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    pub fn GetModuleHandleW(name: *const u16) -> HINSTANCE;
    pub fn GetLocalTime(time: *mut SYSTEMTIME);
    pub fn GlobalAlloc(flags: UINT, bytes: usize) -> HANDLE;
    pub fn GlobalLock(mem: HANDLE) -> *mut c_void;
    pub fn GlobalUnlock(mem: HANDLE) -> BOOL;
    pub fn GlobalFree(mem: HANDLE) -> HANDLE;
    pub fn CreateMutexW(attrs: *mut c_void, owner: BOOL, name: *const u16) -> HANDLE;
    pub fn GetLastError() -> DWORD;
    pub fn GetCurrentProcessId() -> DWORD;
    pub fn GetCurrentProcess() -> HANDLE;
    pub fn AttachConsole(pid: DWORD) -> BOOL;
    pub fn SetStdHandle(which: DWORD, handle: HANDLE) -> BOOL;
    pub fn GetStdHandle(which: DWORD) -> HANDLE;
    pub fn CreateFileW(
        name: *const u16,
        access: DWORD,
        share: DWORD,
        security: *mut c_void,
        disposition: DWORD,
        flags: DWORD,
        template: HANDLE,
    ) -> HANDLE;
}

#[link(name = "shell32")]
extern "system" {
    pub fn Shell_NotifyIconW(message: DWORD, data: *mut NOTIFYICONDATAW) -> BOOL;
    pub fn ShellExecuteW(
        hwnd: HWND,
        operation: *const u16,
        file: *const u16,
        params: *const u16,
        directory: *const u16,
        show: i32,
    ) -> HINSTANCE;
    pub fn SetCurrentProcessExplicitAppUserModelID(app_id: *const u16) -> i32;
}

#[link(name = "ole32")]
extern "system" {
    pub fn CoInitializeEx(reserved: *mut c_void, co_init: DWORD) -> i32;
    pub fn CoUninitialize();
}

#[link(name = "advapi32")]
extern "system" {
    pub fn RegCreateKeyExW(
        key: isize,
        subkey: *const u16,
        reserved: DWORD,
        class: *const u16,
        options: DWORD,
        desired: u32,
        security: *mut c_void,
        result: *mut isize,
        disposition: *mut DWORD,
    ) -> i32;
    pub fn RegOpenKeyExW(
        key: isize,
        subkey: *const u16,
        options: DWORD,
        desired: u32,
        result: *mut isize,
    ) -> i32;
    pub fn RegSetValueExW(
        key: isize,
        name: *const u16,
        reserved: DWORD,
        value_type: DWORD,
        data: *const u8,
        len: DWORD,
    ) -> i32;
    pub fn RegQueryValueExW(
        key: isize,
        name: *const u16,
        reserved: *mut DWORD,
        value_type: *mut DWORD,
        data: *mut u8,
        len: *mut DWORD,
    ) -> i32;
    pub fn RegDeleteValueW(key: isize, name: *const u16) -> i32;
    pub fn RegCloseKey(key: isize) -> i32;
}

// ---------------------------------------------------------------- 基础工具

pub fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// 本地时间，格式 YYYY-MM-DD HH:MM:SS
pub fn local_time() -> String {
    let mut st = SYSTEMTIME::default();
    unsafe { GetLocalTime(&mut st) };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond
    )
}

pub fn instance() -> HINSTANCE {
    unsafe { GetModuleHandleW(std::ptr::null()) }
}

/// 处理标准输出句柄。
///
/// 程序以 GUI 子系统（`windows_subsystem = "windows"`）编译，双击启动时不会创建
/// 控制台窗口；但此时 `GetStdHandle` 返回空句柄，`println!` 会因写入失败而 panic。
/// 这里分两种情况兜住：
///   * 由终端启动 → 挂到父进程的控制台，日志直接打在那个人终端里；
///   * 由资源管理器/开机自启启动 → 把 stdout/stderr 指向 NUL，日志静默丢弃。
/// 必须在任何一次 `println!` 之前调用。
pub fn init_stdio() {
    unsafe {
        let valid = |h: HANDLE| h != 0 && h != INVALID_HANDLE_VALUE;
        let out_ok = valid(GetStdHandle(STD_OUTPUT_HANDLE));
        let err_ok = valid(GetStdHandle(STD_ERROR_HANDLE));
        // 已被终端/管道/文件重定向，保持原样
        if out_ok && err_ok {
            return;
        }

        let attached = AttachConsole(ATTACH_PARENT_PROCESS) != 0;
        let (name, access) = if attached {
            (wide("CONOUT$"), GENERIC_READ | GENERIC_WRITE)
        } else {
            (wide("NUL"), GENERIC_WRITE)
        };
        let handle = CreateFileW(
            name.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            0,
            0,
        );
        if !valid(handle) {
            return;
        }
        if !out_ok {
            SetStdHandle(STD_OUTPUT_HANDLE, handle);
        }
        if !err_ok {
            SetStdHandle(STD_ERROR_HANDLE, handle);
        }
    }
}

/// 从 exe 内嵌资源取应用图标；`size` 传 0 表示按系统图标尺寸。
/// 使用 `LR_SHARED`，图标由系统缓存，调用方不要 DestroyIcon。
pub fn app_icon(size: i32) -> HICON {
    let h = unsafe {
        LoadImageW(
            instance(),
            ICON_RESOURCE_ID as *const u16,
            IMAGE_ICON,
            size,
            size,
            LR_SHARED,
        )
    };
    if h != 0 {
        return h;
    }
    // 资源缺失时退回系统默认图标，保证托盘至少能出现
    eprintln!("[icon] 内嵌图标资源加载失败，退回系统默认图标");
    unsafe { LoadIconW(0, IDI_APPLICATION as *const u16) }
}

/// 通知区图标尺寸随 DPI 变化（100% 为 16px，150% 为 24px）
pub fn tray_icon() -> HICON {
    let size = unsafe {
        let s = GetSystemMetrics(SM_CXSMICON);
        if s > 0 {
            s
        } else {
            16
        }
    };
    app_icon(size)
}

/// 声明本进程的 DPI 感知级别，**必须在创建任何窗口、取任何 DC 之前调用**。
///
/// 之前这段逻辑被放在 `ui_scale()` 里，而 `ui_scale()` 只在弹出第一张卡片时才被
/// 调用——那时托盘窗口早就建好了，`SetProcessDPIAware()` 已经失效。进程于是停留
/// 在 DPI 不感知状态，整张卡片按 96 DPI 画完再被系统位图放大 1.25 倍，字体边缘
/// 自然发虚。现在改为进程启动时一次性声明，且用 Per-Monitor V2。
pub fn init_dpi_awareness() {
    unsafe {
        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) != 0 {
            return;
        }
        // 老系统兜底：至少拿到系统级 DPI 感知，避免位图拉伸
        SetProcessDPIAware();
    }
}

/// 主屏 DPI（96 = 100%）。仅用于计算卡片尺寸，卡片总是出现在主屏工作区。
pub fn system_dpi() -> u32 {
    let dpi = unsafe { GetDpiForSystem() };
    if dpi == 0 {
        96
    } else {
        dpi
    }
}

/// 返回界面缩放系数（96 DPI = 1.0）
pub fn ui_scale() -> f64 {
    system_dpi() as f64 / 96.0
}

pub fn init_com() -> bool {
    let hr = unsafe { CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED) };
    // S_OK(0) 或 S_FALSE(1) 均表示本线程已进入 COM
    hr >= 0
}

pub fn set_aumid(app_id: &str, display_name: &str) {
    let id = wide(app_id);
    unsafe {
        SetCurrentProcessExplicitAppUserModelID(id.as_ptr());
    }

    // 注册 AUMID，让通知中心显示正确的应用名与图标（unpackaged 应用必需）
    let subkey = wide(&format!(r"Software\Classes\AppUserModelId\{app_id}"));
    let mut hkey: isize = 0;
    let rc = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            std::ptr::null(),
            0,
            KEY_WRITE,
            std::ptr::null_mut(),
            &mut hkey,
            std::ptr::null_mut(),
        )
    };
    if rc != 0 {
        return;
    }
    let name = wide("DisplayName");
    let value = wide(display_name);
    unsafe {
        RegSetValueExW(
            hkey,
            name.as_ptr(),
            0,
            REG_SZ,
            value.as_ptr() as *const u8,
            (value.len() * 2) as DWORD,
        );
        let icon = std::env::current_exe()
            .map(|p| wide(&p.to_string_lossy()))
            .unwrap_or_default();
        if !icon.is_empty() {
            let icon_name = wide("IconUri");
            RegSetValueExW(
                hkey,
                icon_name.as_ptr(),
                0,
                REG_SZ,
                icon.as_ptr() as *const u8,
                (icon.len() * 2) as DWORD,
            );
        }
        let show = wide("ShowInSettings");
        let one: DWORD = 1;
        RegSetValueExW(
            hkey,
            show.as_ptr(),
            0,
            REG_DWORD,
            &one as *const DWORD as *const u8,
            4,
        );
        RegCloseKey(hkey);
    }
}

/// 单实例：第一个进程持有互斥体直到退出
pub fn claim_single_instance(name: &str) -> bool {
    let n = wide(name);
    unsafe {
        let h = CreateMutexW(std::ptr::null_mut(), 1, n.as_ptr());
        h != 0 && GetLastError() != ERROR_ALREADY_EXISTS
    }
}

/// 按窗口类名找已有的实例窗口（用于"又来一个实例"时给它发消息）
pub fn find_window(class: &str) -> HWND {
    let c = wide(class);
    unsafe { FindWindowW(c.as_ptr(), std::ptr::null()) }
}

pub fn open_url(url: &str) {
    let file = wide(url);
    let open = wide("open");
    unsafe {
        ShellExecuteW(
            0,
            open.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            5, // SW_SHOW
        );
    }
}

/// 写 Unicode 文本到剪贴板。必须在 STA 线程、且不被其他进程占用时调用。
pub fn clipboard_set(text: &str) -> Result<(), String> {
    let mut units = wide(text);
    let bytes = units.len() * 2;
    unsafe {
        for attempt in 0..8 {
            if OpenClipboard(0) != 0 {
                break;
            }
            if attempt == 7 {
                return Err("剪贴板被其他程序占用".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        EmptyClipboard();
        let mem = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if mem == 0 {
            CloseClipboard();
            return Err("GlobalAlloc 失败".into());
        }
        let ptr = GlobalLock(mem) as *mut u16;
        if ptr.is_null() {
            GlobalFree(mem);
            CloseClipboard();
            return Err("GlobalLock 失败".into());
        }
        std::ptr::copy_nonoverlapping(units.as_mut_ptr(), ptr, units.len());
        GlobalUnlock(mem);
        if SetClipboardData(CF_UNICODETEXT, mem) == 0 {
            GlobalFree(mem);
            CloseClipboard();
            return Err("SetClipboardData 失败".into());
        }
        // 成功后内存所有权归系统，不能 GlobalFree
        CloseClipboard();
    }
    Ok(())
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "SmsSink";

fn reg_set_string(key: isize, name: &str, value: &str) -> Result<(), String> {
    let n = wide(name);
    let v = wide(value);
    let rc = unsafe {
        RegSetValueExW(
            key,
            n.as_ptr(),
            0,
            REG_SZ,
            v.as_ptr() as *const u8,
            (v.len() * 2) as DWORD,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(format!("RegSetValueExW 返回 {rc}"))
    }
}

/// 开机自启写 HKCU\...\Run，不需要管理员权限
pub fn set_auto_start(enabled: bool) -> Result<(), String> {
    let sub = wide(RUN_KEY);
    let mut hkey: isize = 0;
    let rc = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            0,
            KEY_WRITE | KEY_QUERY_VALUE,
            &mut hkey,
        )
    };
    if rc != 0 {
        return Err(format!("无法打开 Run 键: {rc}"));
    }
    let result = if enabled {
        let exe = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into_owned();
        reg_set_string(hkey, RUN_VALUE, &format!("\"{exe}\""))
    } else {
        let name = wide(RUN_VALUE);
        let rc = unsafe { RegDeleteValueW(hkey, name.as_ptr()) };
        if rc == 0 || rc == 2 {
            Ok(())
        } else {
            Err(format!("RegDeleteValueW 返回 {rc}"))
        }
    };
    unsafe {
        RegCloseKey(hkey);
    }
    result
}

pub fn auto_start_enabled() -> bool {
    let sub = wide(RUN_KEY);
    let mut hkey: isize = 0;
    let rc = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            0,
            KEY_QUERY_VALUE,
            &mut hkey,
        )
    };
    if rc != 0 {
        return false;
    }
    let name = wide(RUN_VALUE);
    let mut size: DWORD = 0;
    let found = unsafe {
        RegQueryValueExW(
            hkey,
            name.as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    } == 0;
    unsafe {
        RegCloseKey(hkey);
    }
    found
}

// ---------- 托盘气泡 / 对话框 / 菜单拼装 ----------
//
// 托盘菜单和气泡是「看不见的控制台」唯一的反馈通道：程序以 GUI 子系统启动时
// stdout 被丢进 NUL，任何只写日志的提示等于没有。下面三个小工具就是为了让
// 每个用户动作都有可见回执。

/// 托盘图标的 uID，Shell_NotifyIcon 的所有调用（含气泡）必须一致
pub const TRAY_ICON_ID: UINT = 1;
pub const NIM_MODIFY: DWORD = 1;

/// 把字符串写进定长宽字符数组并补 NUL，超长截断。
fn fill_wide(dest: &mut [u16], text: &str) {
    let mut n = 0;
    for unit in text.encode_utf16().take(dest.len().saturating_sub(1)) {
        dest[n] = unit;
        n += 1;
    }
    dest[n] = 0;
}

/// 在托盘图标上弹气泡提示。`warning` 为 true 时用警告图标。
pub fn notify_balloon(hwnd: HWND, title: &str, text: &str, warning: bool) {
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as DWORD;
    data.hWnd = hwnd;
    data.uID = TRAY_ICON_ID;
    data.uFlags = NIF_INFO;
    data.dwInfoFlags = if warning { NIIF_WARNING } else { NIIF_INFO } | NIIF_NOSOUND;
    fill_wide(&mut data.szInfoTitle, title);
    fill_wide(&mut data.szInfo, text);
    unsafe {
        Shell_NotifyIconW(NIM_MODIFY, &mut data);
    }
}

/// 阻塞式提示框，仅用于没有其它反馈通道的致命错误（如端口被占用）。
pub fn error_box(text: &str, caption: &str) {
    let t = wide(text);
    let c = wide(caption);
    unsafe {
        MessageBoxW(
            0,
            t.as_ptr(),
            c.as_ptr(),
            MB_OK | MB_ICONERROR | MB_TOPMOST | MB_SETFOREGROUND,
        );
    }
}

/// 普通菜单项（点击后返回 id）
pub fn menu_item(menu: HMENU, flags: UINT, id: usize, text: &str) {
    let t = wide(text);
    unsafe {
        AppendMenuW(menu, flags | MF_STRING, id, t.as_ptr());
    }
}

/// 状态行：不可点击的说明文字，用来在菜单里展示「今日 12 条 · 监听 8787」
pub fn menu_status(menu: HMENU, text: &str) {
    menu_item(menu, MF_DISABLED | MF_GRAYED, 0, text);
}

pub fn menu_separator(menu: HMENU) {
    unsafe {
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
    }
}

/// 挂子菜单
pub fn menu_popup(menu: HMENU, sub: HMENU, text: &str) {
    let t = wide(text);
    unsafe {
        AppendMenuW(menu, MF_POPUP, sub as usize, t.as_ptr());
    }
}
