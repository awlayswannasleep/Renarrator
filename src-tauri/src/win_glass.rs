//! Windows glass: сильный акриловый блюр + скруглённые углы.
//!
//! 1. **Сильный блюр** через недокументированный `SetWindowCompositionAttribute`
//!    (`user32.dll`) с `ACCENT_ENABLE_ACRYLICBLURBEHIND` — тот же мощный акрил,
//!    что и у трея/Пуска Windows 10/11. Старый `DwmEnableBlurBehindWindow` (Aero)
//!    даёт крошечный блюр («просвечивает стекло») — оставлен как fallback.
//! 2. **Скруглённые углы** — `SetWindowRgn` со скруглённым регионом: углы за его
//!    пределами по-настоящему прозрачные (окно transparent + decorations: false).

use std::ffi::c_void;
use tauri::WebviewWindow;
use windows_sys::Win32::Foundation::{FALSE, HWND, TRUE};
use windows_sys::Win32::Graphics::Dwm::{
    DwmEnableBlurBehindWindow, DWM_BB_BLURREGION, DWM_BB_ENABLE, DWM_BLURBEHIND,
};
use windows_sys::Win32::Graphics::Gdi::{CreateRoundRectRgn, DeleteObject, HRGN, SetWindowRgn};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_STYLE, HWND_TOP, SWP_FRAMECHANGED,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_CAPTION, WS_THICKFRAME,
};

const WCA_ACCENT_POLICY: u32 = 19;

#[allow(non_snake_case, dead_code)]
mod accent_state {
    pub const ACCENT_DISABLED: u32 = 0;
    pub const ACCENT_ENABLE_GRADIENT: u32 = 1;
    pub const ACCENT_ENABLE_TRANSPARENTGRADIENT: u32 = 2;
    pub const ACCENT_ENABLE_BLURBEHIND: u32 = 3;
    pub const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;
    pub const ACCENT_ENABLE_HOSTBACKDROP: u32 = 5; // Windows 11 22H2+
}

#[repr(C)]
#[derive(Clone, Copy)]
struct AccentPolicy {
    accent_state: u32,
    accent_flags: u32,
    gradient_color: u32, // ABGR (альфа в старшем байте)
    animation_id: u32,
}

#[repr(C)]
struct WindowCompositionAttributeData {
    attribute: u32,
    data: *mut c_void,
    size: usize,
}

type SetWindowCompositionAttributeFn =
    unsafe extern "system" fn(HWND, *mut WindowCompositionAttributeData) -> i32;

/// Символ резолвим один раз и кэшируем (thread-safe).
fn load_accent_api() -> Option<SetWindowCompositionAttributeFn> {
    use std::sync::OnceLock;
    static FN_PTR: OnceLock<Option<SetWindowCompositionAttributeFn>> = OnceLock::new();
    *FN_PTR.get_or_init(|| unsafe {
        let user32 = GetModuleHandleA(c"user32.dll".as_ptr().cast());
        if user32.is_null() {
            return None;
        }
        let proc = GetProcAddress(user32, c"SetWindowCompositionAttribute".as_ptr().cast());
        proc.map(|p| std::mem::transmute::<_, SetWindowCompositionAttributeFn>(p))
    })
}

/// Акриловый блюр. `tint_abgr` — тонировка (0xAA_BB_GG_RR); маленькая альфа
/// сохраняет максимальную «стеклянность».
fn apply_acrylic(hwnd: HWND, tint_abgr: u32) -> Result<(), String> {
    let f = load_accent_api().ok_or("SetWindowCompositionAttribute not found")?;
    let mut policy = AccentPolicy {
        accent_state: accent_state::ACCENT_ENABLE_ACRYLICBLURBEHIND,
        accent_flags: 0,
        gradient_color: tint_abgr,
        animation_id: 0,
    };
    let mut data = WindowCompositionAttributeData {
        attribute: WCA_ACCENT_POLICY,
        data: &mut policy as *mut _ as *mut c_void,
        size: std::mem::size_of::<AccentPolicy>(),
    };
    if unsafe { f(hwnd, &mut data) } == 0 {
        Err("SetWindowCompositionAttribute returned 0".into())
    } else {
        Ok(())
    }
}

/// Старый Aero-блюр как fallback (слабый, но лучше, чем ничего).
fn apply_legacy_blur(hwnd: HWND) -> Result<(), String> {
    let bb = DWM_BLURBEHIND {
        dwFlags: DWM_BB_ENABLE | DWM_BB_BLURREGION,
        fEnable: TRUE,
        hRgnBlur: std::ptr::null_mut(),
        fTransitionOnMaximized: FALSE,
    };
    let hr = unsafe { DwmEnableBlurBehindWindow(hwnd, &bb) };
    if hr != 0 {
        Err(format!("DwmEnableBlurBehindWindow HRESULT=0x{hr:08x}"))
    } else {
        Ok(())
    }
}

/// «Жидкое стекло»: сильный акриловый блюр фона + скруглённый регион (logical px).
/// Регион пере-применяется при каждом перемещении/смене DPI: DWM иногда сбрасывает
/// SetWindowRgn, из-за чего по углам «просвечивают» квадратные острые уголки.
pub fn apply_rounded_blur(window: &WebviewWindow, radius_logical: f64) {
    if let Err(e) = try_apply(window, radius_logical) {
        eprintln!("[glass] blur not applied: {e}");
    }
}

/// Применить только скруглённый регион (без блюра). Вызывается на старте и на каждое
/// перемещение окна, чтобы углы оставались честно круглыми.
fn try_apply(window: &WebviewWindow, radius_logical: f64) -> Result<(), String> {
    let hwnd = window.hwnd().map_err(|e| e.to_string())?.0 as HWND;
    apply_region(window, radius_logical)?;

    // Тонировка почти прозрачная (альфа ~0x12) — стекло остаётся воздушным.
    if let Err(e) = apply_acrylic(hwnd, 0x1214_1414) {
        eprintln!("[glass] acrylic unavailable ({e}), falling back to DWM blur");
        apply_legacy_blur(hwnd)?;
    }
    Ok(())
}

/// Установить скруглённый регион окна (углы за его пределами — прозрачные).
/// Публично: вызывается повторно из обработчика Moved/Resized, т.к. DWM может
/// сбросить регион, и тогда по углам снова видны острые квадратные уголки.
pub fn apply_region(window: &WebviewWindow, radius_logical: f64) -> Result<(), String> {
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let size = window.inner_size().map_err(|e| e.to_string())?;
    let hwnd = window.hwnd().map_err(|e| e.to_string())?.0 as HWND;

    let w = size.width as i32;
    let h = size.height as i32;
    // Радиус региона задаётся как диаметр эллипса (физические пиксели).
    let d = ((radius_logical * scale).round() as i32 * 2).max(2);

    unsafe {
        let hrgn: HRGN = CreateRoundRectRgn(0, 0, w + 1, h + 1, d, d);
        if hrgn.is_null() {
            return Err("CreateRoundRectRgn returned NULL".into());
        }
        // После SetWindowRgn владение регионом переходит ОС — НЕ удалять вручную.
        if SetWindowRgn(hwnd, hrgn, TRUE) == 0 {
            DeleteObject(hrgn);
            return Err("SetWindowRgn failed".into());
        }
    }
    Ok(())
}

/// Снять нативную рамку/титлбар Windows (WS_CAPTION | WS_THICKFRAME).
///
/// Даже с `decorations(false)` WebView2 иногда отрисовывает нативную полосу
/// заголовка поверх содержимого (особенно при первом показе или смене DPI).
/// Жёстко вычищаем стили рамки и просим DWM пересчитать non-client область —
/// после этого полоса физически не может появиться.
pub fn strip_native_chrome(window: &WebviewWindow) {
    if let Err(e) = try_strip(window) {
        eprintln!("[glass] strip_native_chrome failed: {e}");
    }
}

fn try_strip(window: &WebviewWindow) -> Result<(), String> {
    let hwnd = window.hwnd().map_err(|e| e.to_string())?.0 as HWND;
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        if style == 0 {
            return Err("GetWindowLongPtrW returned 0".into());
        }
        let cleaned = style & !(WS_CAPTION as isize) & !(WS_THICKFRAME as isize);
        if cleaned != style {
            SetWindowLongPtrW(hwnd, GWL_STYLE, cleaned);
            // SWP_FRAMECHANGED заставляет применить новый стиль и пересчитать рамку.
            SetWindowPos(
                hwnd,
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
            );
        }
    }
    Ok(())
}
