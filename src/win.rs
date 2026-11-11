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
pub const WM_TIMER: UINT = 0x0113;
pub const WM_RBUTTONUP: UINT = 0x0205;
pub const WM_LBUTTONUP: UINT = 0x0202;
pub const WM_LBUTTONDBLCLK: UINT = 0x0203;
pub const WM_CTLCOLORSTATIC: UINT = 0x0138;

pub const WS_CHILD: DWORD = 0x4000_0000;
pub const WS_VISIBLE: DWORD = 0x1000_0000;
pub const WS_POPUP: DWORD = 0x8000_0000;
pub const WS_TABSTOP: DWORD = 0x0001_0000;
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
pub const NOTIFYICON_VERSION_4: UINT = 4;

pub const MF_STRING: UINT = 0x0000;
pub const MF_CHECKED: UINT = 0x0008;
pub const MF_UNCHECKED: UINT = 0x0000;
pub const MF_SEPARATOR: UINT = 0x0800;
pub const MF_GRAYED: UINT = 0x0001;
pub const TPM_RIGHTBUTTON: UINT = 0x0002;
pub const TPM_RETURNCMD: UINT = 0x0100;
pub const TPM_NONOTIFY: UINT = 0x0080;

pub const IDI_APPLICATION: usize = 32512;
pub const IDC_ARROW: usize = 32512;
pub const SPI_GETWORKAREA: UINT = 0x0030;
pub const SM_CXSCREEN: i32 = 0;

pub const DT_LEFT: UINT = 0x0000;
pub const DT_CENTER: UINT = 0x0001;
pub const DT_VCENTER: UINT = 0x0004;
pub const DT_SINGLELINE: UINT = 0x0020;
pub const DT_WORD_ELLIPSIS: UINT = 0x0004_0000;
pub const DT_NOPREFIX: UINT = 0x0000_0800;
pub const TRANSPARENT: i32 = 1;
pub const FW_NORMAL: i32 = 400;
pub const FW_BOLD: i32 = 700;
pub const DEFAULT_CHARSET: u32 = 1;
pub const CLEARTYPE_QUALITY: u32 = 5;
pub const DEFAULT_PITCH: u32 = 0;
pub const FF_DONTCARE: u32 = 0;
pub const DEFAULT_GUI_FONT: i32 = 17;
pub const CF_UNICODETEXT: UINT = 13;
pub const GMEM_MOVEABLE: UINT = 0x0002;
pub const COINIT_APARTMENTTHREADED: DWORD = 0x2;
pub const ERROR_ALREADY_EXISTS: DWORD = 183;
pub const HKEY_CURRENT_USER: isize = 0x8000_0001_u32 as i32 as isize;
pub const KEY_WRITE: u32 = 0x20006;
pub const KEY_QUERY_VALUE: u32 = 0x1;
pub const REG_SZ: DWORD = 1;
pub const REG_DWORD: DWORD = 4;

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
    pub fn BeginPaint(hwnd: HWND, ps: *mut PAINTSTRUCT) -> HDC;
    pub fn EndPaint(hwnd: HWND, ps: *const PAINTSTRUCT) -> BOOL;
    pub fn FillRect(hdc: HDC, rect: *const RECT, brush: HBRUSH) -> i32;
    pub fn DrawTextW(hdc: HDC, text: *const u16, count: i32, rect: *mut RECT, format: UINT) -> i32;
    pub fn LoadIconW(hInstance: HINSTANCE, name: *const u16) -> HICON;
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
    pub fn GetDpiForSystem() -> UINT;
    pub fn OpenClipboard(hwnd: HWND) -> BOOL;
    pub fn EmptyClipboard() -> BOOL;
    pub fn SetClipboardData(format: UINT, mem: HANDLE) -> HANDLE;
    pub fn CloseClipboard() -> BOOL;
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
    pub fn SetBkMode(hdc: HDC, mode: i32) -> i32;
    pub fn SetTextColor(hdc: HDC, color: COLORREF) -> COLORREF;
    pub fn SelectObject(hdc: HDC, obj: HGDIOBJ) -> HGDIOBJ;
    pub fn DeleteObject(obj: HGDIOBJ) -> BOOL;
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

/// 返回界面缩放系数（96 DPI = 1.0）
pub fn ui_scale() -> f64 {
    unsafe {
        SetProcessDPIAware();
        let dpi = GetDpiForSystem();
        if dpi == 0 {
            1.0
        } else {
            dpi as f64 / 96.0
        }
    }
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
    let subkey = wide(&format!(
        r"Software\Classes\AppUserModelId\{app_id}"
    ));
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
