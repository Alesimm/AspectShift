use std::ffi::c_void;
use std::mem::zeroed;

use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, IsIconic, IsWindowVisible, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    GWL_EXSTYLE, GWL_STYLE, HWND_TOP, SWP_FRAMECHANGED, SWP_NOOWNERZORDER, SWP_NOZORDER,
    SWP_SHOWWINDOW, SW_RESTORE, WS_BORDER, WS_CAPTION, WS_DLGFRAME, WS_EX_CLIENTEDGE,
    WS_EX_DLGMODALFRAME, WS_EX_STATICEDGE, WS_EX_WINDOWEDGE, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
    WS_POPUP, WS_SYSMENU, WS_THICKFRAME, WS_VISIBLE,
};

/// Representa una ventana de aplicación visible candidata a ajuste sin bordes.
#[derive(Debug, Clone)]
pub struct CandidateWindow {
    pub hwnd: isize,
    pub title: String,
}

fn read_window_title(hwnd: HWND) -> Option<String> {
    unsafe {
        if IsWindowVisible(hwnd) == 0 {
            return None;
        }
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 || len > 256 {
            return None;
        }
        let mut buf = vec![0u16; (len + 1) as usize];
        let copied = GetWindowTextW(hwnd, buf.as_mut_ptr(), len + 1);
        if copied <= 0 {
            return None;
        }
        let title = String::from_utf16_lossy(&buf[..copied as usize])
            .trim()
            .to_string();
        if title.is_empty() || is_ignored_window(&title) {
            return None;
        }
        Some(title)
    }
}

/// Excluye la propia ventana de AspectShift y ventanas internas del sistema.
fn is_ignored_window(title: &str) -> bool {
    let lower = title.to_lowercase();
    lower.contains("aspectshift")
        || lower == "program manager"
        || lower == "settings"
        || lower == "configuración"
        || lower == "windows input experience"
        || lower == "experiencia de entrada de windows"
        || lower == "microsoft text input application"
}

unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if let Some(title) = read_window_title(hwnd) {
        let list = &mut *(lparam as *mut Vec<CandidateWindow>);
        list.push(CandidateWindow {
            hwnd: hwnd as isize,
            title,
        });
    }
    1
}

/// Enumera todas las ventanas visibles ordenadas alfabéticamente.
pub fn scan_candidate_windows() -> Vec<CandidateWindow> {
    let mut list: Vec<CandidateWindow> = Vec::new();
    unsafe {
        let ptr = &mut list as *mut Vec<CandidateWindow> as LPARAM;
        EnumWindows(Some(enum_windows_proc), ptr);
    }
    list.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    list
}

/// Devuelve únicamente la ventana actualmente en primer plano (si no es AspectShift ni del sistema).
pub fn find_foreground_window() -> Option<CandidateWindow> {
    unsafe {
        let fg = GetForegroundWindow();
        if !fg.is_null() {
            if let Some(title) = read_window_title(fg) {
                return Some(CandidateWindow {
                    hwnd: fg as isize,
                    title,
                });
            }
        }
    }
    None
}

/// Obtiene el centro actual de una ventana para determinar en qué monitor se encuentra.
pub fn get_window_center(hwnd_val: isize) -> Option<(i32, i32)> {
    let hwnd = hwnd_val as *mut c_void;
    if hwnd.is_null() {
        return None;
    }
    unsafe {
        let mut rect: RECT = zeroed();
        if GetWindowRect(hwnd, &mut rect) != 0 {
            let cx = (rect.left + rect.right) / 2;
            let cy = (rect.top + rect.bottom) / 2;
            return Some((cx, cy));
        }
    }
    None
}

/// Quita los bordes y barra de título de la ventana indicada y la ajusta a las coordenadas del monitor.
pub fn apply_borderless_window(
    hwnd_val: isize,
    title_hint: &str,
    mon_x: i32,
    mon_y: i32,
    mon_w: u32,
    mon_h: u32,
) -> Result<String, String> {
    let hwnd = hwnd_val as *mut c_void;
    if hwnd.is_null() {
        return Err("Ventana no válida.".to_string());
    }

    unsafe {
        if IsWindowVisible(hwnd) == 0 {
            return Err(format!("La ventana '{}' ya no está visible.", title_hint));
        }

        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }

        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let strip_mask = WS_CAPTION
            | WS_THICKFRAME
            | WS_MINIMIZEBOX
            | WS_MAXIMIZEBOX
            | WS_SYSMENU
            | WS_DLGFRAME
            | WS_BORDER;
        let new_style = ((style & !strip_mask) | WS_POPUP | WS_VISIBLE) as isize;
        SetWindowLongPtrW(hwnd, GWL_STYLE, new_style);

        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let ex_strip =
            WS_EX_DLGMODALFRAME | WS_EX_CLIENTEDGE | WS_EX_STATICEDGE | WS_EX_WINDOWEDGE;
        let new_ex_style = (ex_style & !ex_strip) as isize;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_ex_style);

        let ok = SetWindowPos(
            hwnd,
            HWND_TOP,
            mon_x,
            mon_y,
            mon_w as i32,
            mon_h as i32,
            SWP_FRAMECHANGED | SWP_SHOWWINDOW | SWP_NOZORDER | SWP_NOOWNERZORDER,
        );

        if ok == 0 {
            return Err(format!(
                "No se pudo ajustar '{}'. Si corre elevado, abre AspectShift como Administrador.",
                title_hint
            ));
        }

        Ok(format!(
            "Ventana sin bordes aplicada a '{}' ({}×{})",
            title_hint, mon_w, mon_h
        ))
    }
}
