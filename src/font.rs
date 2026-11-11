//! 字体选型与文本度量。
//!
//! 这里解决 Win32 自绘最常见的两个「看着廉价」的根源：
//!
//! 1. **中文掉到宋体**。若 `CreateFontW` 只请求 `Segoe UI`，它没有汉字字形，
//!    GDI 的字体链接（FontLink）会把每个汉字替换成宋体——宋体是点阵时代的
//!    衬线字体，小字号下笔画细、粗细不匀，和旁边的 Segoe UI 数字完全是两种
//!    气质。所以这里优先选同时包含中英字形的中文 UI 字体，整套界面只用一套字形。
//! 2. **尺寸按 96 DPI 写死**。缩放到 125% 时要么字号偏小，要么被系统位图拉伸
//!    变糊。这里统一用「逻辑像素 × dpi / 96」换算成设备像素，再交给 GDI。
use crate::win::*;
use std::sync::OnceLock;

/// 候选字体，按优先级排列。
///
/// * `Microsoft YaHei UI`：Win8.1+ 系统 UI 字体，中英混排的紧排版本，也是资源
///   管理器、设置等系统界面的实际用字，小字号下比 `Microsoft YaHei` 更精神；
/// * `Microsoft YaHei`：没有 UI 变体时的退路；
/// * `Segoe UI`：英文系统上的兜底（中文会走字体链接，但那时已经没有更好的选择）。
const CANDIDATES: [&str; 3] = ["Microsoft YaHei UI", "Microsoft YaHei", "Segoe UI"];

static FACE: OnceLock<Vec<u16>> = OnceLock::new();

/// 当前系统上实际可用的界面字体名（宽字符，直接喂给 `CreateFontW`）。
pub fn ui_face() -> &'static [u16] {
    FACE.get_or_init(|| {
        for name in CANDIDATES {
            if installed(name) {
                return wide(name);
            }
        }
        wide("Segoe UI")
    })
}

/// 系统里是否真的装了这个字族。
///
/// 不能靠 `CreateFontW` 的返回值判断——GDI 找不到字体会静默替换成别的字体并
/// 返回一个有效句柄，正是这个「静默替换」造成了字体错配。所以用枚举接口确认。
fn installed(face: &str) -> bool {
    let mut lf: LOGFONTW = unsafe { std::mem::zeroed() };
    lf.lfCharSet = DEFAULT_CHARSET as u8;
    lf.lfWeight = FW_NORMAL;
    for (slot, unit) in lf.lfFaceName.iter_mut().zip(face.encode_utf16()) {
        *slot = unit;
    }

    unsafe extern "system" fn first_only(
        _lf: *const LOGFONTW,
        _tm: *const std::ffi::c_void,
        _kind: DWORD,
        param: LPARAM,
    ) -> i32 {
        *(param as *mut bool) = true;
        0 // 返回 0：找到一个就够了，停止枚举
    }

    let mut found = false;
    unsafe {
        let hdc = CreateCompatibleDC(0);
        if hdc == 0 {
            return false;
        }
        EnumFontFamiliesExW(
            hdc,
            &lf,
            Some(first_only),
            &mut found as *mut bool as LPARAM,
            0,
        );
        DeleteDC(hdc);
    }
    found
}

/// 按逻辑像素创建字体：`px` 是 96 DPI 下的设计尺寸，`dpi` 是目标设备 DPI。
pub fn create(face: &[u16], px: f64, dpi: u32, weight: i32) -> HFONT {
    let height = -((px * dpi as f64 / 96.0).round() as i32);
    unsafe {
        CreateFontW(
            height,
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
