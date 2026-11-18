use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreatePen, CreateSolidBrush, DeleteObject, DrawTextW, Ellipse,
    EndPaint, FillRect, InvalidateRect, RoundRect, SelectObject, SetBkMode, SetTextColor,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DT_LEFT,
    DT_SINGLELINE, DT_VCENTER, FF_SWISS, FW_SEMIBOLD, OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_SOLID,
    TRANSPARENT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW, KillTimer,
    PostMessageW, RegisterClassExW, SetLayeredWindowAttributes, SetTimer, SetWindowPos,
    ShowWindow, TranslateMessage, HWND_TOPMOST, LWA_ALPHA, MSG, SWP_NOACTIVATE, SWP_NOZORDER,
    SWP_SHOWWINDOW, SW_HIDE, WM_APP, WM_DESTROY, WM_PAINT, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const WM_APP_SHOW_TOAST: u32 = WM_APP + 1;
const TIMER_ANIM_ID: usize = 1;

const TOAST_W: i32 = 248;
const TOAST_H: i32 = 38;
const TOP_MARGIN: i32 = 24;

// Duraciones de animación en milisegundos
const FADE_IN_MS: f32 = 200.0;
const HOLD_MS: f32 = 1750.0;
const FADE_OUT_MS: f32 = 350.0;

#[derive(Clone)]
struct ToastPayload {
    mon_x: i32,
    mon_y: i32,
    mon_w: i32,
    text: String,
    is_native: bool,
    started_at: Instant,
}

/// Estado compartido entre el hilo principal y el hilo de la notificación flotante.
struct SharedToastState {
    payload: Mutex<Option<ToastPayload>>,
    hwnd: AtomicIsize,
}

/// Notificación flotante minimalista en pantalla que no roba foco ni bloquea clics.
pub struct ScreenToast {
    shared: Arc<SharedToastState>,
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn color_rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

thread_local! {
    static THREAD_SHARED: std::cell::RefCell<Option<Arc<SharedToastState>>> =
        const { std::cell::RefCell::new(None) };
}

unsafe extern "system" fn toast_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_APP_SHOW_TOAST => {
            THREAD_SHARED.with(|cell| {
                if let Some(shared) = cell.borrow().as_ref() {
                    if let Ok(guard) = shared.payload.lock() {
                        if let Some(p) = guard.as_ref() {
                            let x = p.mon_x + (p.mon_w - TOAST_W) / 2;
                            let y = p.mon_y + TOP_MARGIN + 8;
                            SetLayeredWindowAttributes(hwnd, 0, 1, LWA_ALPHA);
                            SetWindowPos(
                                hwnd,
                                HWND_TOPMOST,
                                x,
                                y,
                                TOAST_W,
                                TOAST_H,
                                SWP_NOACTIVATE | SWP_SHOWWINDOW,
                            );
                            InvalidateRect(hwnd, null(), 1);
                            SetTimer(hwnd, TIMER_ANIM_ID, 16, None);
                        }
                    }
                }
            });
            0
        }
        WM_TIMER if wparam == TIMER_ANIM_ID => {
            THREAD_SHARED.with(|cell| {
                if let Some(shared) = cell.borrow().as_ref() {
                    let snapshot = shared.payload.lock().ok().and_then(|g| g.clone());
                    if let Some(p) = snapshot {
                        let elapsed = p.started_at.elapsed().as_secs_f32() * 1000.0;
                        let total = FADE_IN_MS + HOLD_MS + FADE_OUT_MS;

                        if elapsed >= total {
                            KillTimer(hwnd, TIMER_ANIM_ID);
                            ShowWindow(hwnd, SW_HIDE);
                            return;
                        }

                        let (alpha, slide_offset) = if elapsed < FADE_IN_MS {
                            let t = (elapsed / FADE_IN_MS).clamp(0.0, 1.0);
                            let ease = 1.0 - (1.0 - t).powi(3);
                            ((236.0 * ease) as u8, ((1.0 - ease) * 8.0).round() as i32)
                        } else if elapsed < FADE_IN_MS + HOLD_MS {
                            (236u8, 0)
                        } else {
                            let t = ((elapsed - FADE_IN_MS - HOLD_MS) / FADE_OUT_MS).clamp(0.0, 1.0);
                            let ease = t * t;
                            ((236.0 * (1.0 - ease)) as u8, (-(ease * 4.0)).round() as i32)
                        };

                        let x = p.mon_x + (p.mon_w - TOAST_W) / 2;
                        let y = p.mon_y + TOP_MARGIN + slide_offset;
                        SetLayeredWindowAttributes(hwnd, 0, alpha.max(1), LWA_ALPHA);
                        SetWindowPos(
                            hwnd,
                            HWND_TOPMOST,
                            x,
                            y,
                            TOAST_W,
                            TOAST_H,
                            SWP_NOACTIVATE | SWP_NOZORDER,
                        );
                    }
                }
            });
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rc: RECT = zeroed();
            GetClientRect(hwnd, &mut rc);

            // Fondo grafito oscuro
            let bg_brush = CreateSolidBrush(color_rgb(23, 25, 30));
            FillRect(hdc, &rc, bg_brush);

            // Borde sutil redondeado interno
            let border_pen = CreatePen(PS_SOLID, 1, color_rgb(52, 58, 72));
            let old_pen = SelectObject(hdc, border_pen);
            let old_brush = SelectObject(hdc, bg_brush);
            RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, 14, 14);

            // Punto de estado (verde salvia para nativa, azul índigo para cambio de relación)
            let is_native = THREAD_SHARED.with(|cell| {
                cell.borrow()
                    .as_ref()
                    .and_then(|s| s.payload.lock().ok().and_then(|g| g.as_ref().map(|p| p.is_native)))
                    .unwrap_or(false)
            });
            let dot_col = if is_native {
                color_rgb(86, 182, 159)
            } else {
                color_rgb(110, 142, 247)
            };
            let dot_brush = CreateSolidBrush(dot_col);
            let dot_pen = CreatePen(PS_SOLID, 1, dot_col);
            SelectObject(hdc, dot_brush);
            SelectObject(hdc, dot_pen);
            let cy = (rc.top + rc.bottom) / 2;
            Ellipse(hdc, 15, cy - 4, 23, cy + 4);

            // Texto limpio en Segoe UI
            let font_name = to_wide("Segoe UI");
            let font = CreateFontW(
                16,
                0,
                0,
                0,
                FW_SEMIBOLD as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32,
                CLIP_DEFAULT_PRECIS as u32,
                CLEARTYPE_QUALITY as u32,
                (DEFAULT_PITCH | FF_SWISS) as u32,
                font_name.as_ptr(),
            );
            let old_font = SelectObject(hdc, font);
            SetBkMode(hdc, TRANSPARENT as i32);
            SetTextColor(hdc, color_rgb(236, 238, 242));

            let text = THREAD_SHARED.with(|cell| {
                cell.borrow()
                    .as_ref()
                    .and_then(|s| {
                        s.payload
                            .lock()
                            .ok()
                            .and_then(|g| g.as_ref().map(|p| p.text.clone()))
                    })
                    .unwrap_or_default()
            });
            let mut wide_text: Vec<u16> = text.encode_utf16().collect();
            let mut text_rc = RECT {
                left: 33,
                top: rc.top,
                right: rc.right - 12,
                bottom: rc.bottom,
            };
            if !wide_text.is_empty() {
                DrawTextW(
                    hdc,
                    wide_text.as_mut_ptr(),
                    wide_text.len() as i32,
                    &mut text_rc,
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE,
                );
            }

            SelectObject(hdc, old_font);
            SelectObject(hdc, old_pen);
            SelectObject(hdc, old_brush);
            DeleteObject(font);
            DeleteObject(dot_pen);
            DeleteObject(dot_brush);
            DeleteObject(border_pen);
            DeleteObject(bg_brush);

            EndPaint(hwnd, &ps);
            0
        }
        WM_DESTROY => 0,
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

impl ScreenToast {
    pub fn new() -> Self {
        let shared = Arc::new(SharedToastState {
            payload: Mutex::new(None),
            hwnd: AtomicIsize::new(0),
        });

        let thread_shared = Arc::clone(&shared);
        thread::Builder::new()
            .name("aspectshift-toast".to_string())
            .spawn(move || unsafe {
                THREAD_SHARED.with(|cell| {
                    *cell.borrow_mut() = Some(Arc::clone(&thread_shared));
                });

                let class_name = to_wide("AspectShiftToastClass");
                let wc = WNDCLASSEXW {
                    cbSize: size_of::<WNDCLASSEXW>() as u32,
                    style: 0,
                    lpfnWndProc: Some(toast_wnd_proc),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: null_mut(),
                    hIcon: null_mut(),
                    hCursor: null_mut(),
                    hbrBackground: null_mut(),
                    lpszMenuName: null(),
                    lpszClassName: class_name.as_ptr(),
                    hIconSm: null_mut(),
                };
                RegisterClassExW(&wc);

                let win_title = to_wide("AspectShiftToast");
                let hwnd = CreateWindowExW(
                    WS_EX_LAYERED
                        | WS_EX_TOPMOST
                        | WS_EX_TOOLWINDOW
                        | WS_EX_NOACTIVATE
                        | WS_EX_TRANSPARENT,
                    class_name.as_ptr(),
                    win_title.as_ptr(),
                    WS_POPUP,
                    0,
                    0,
                    TOAST_W,
                    TOAST_H,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    null(),
                );

                if !hwnd.is_null() {
                    // Esquinas redondeadas nativas en Windows 11
                    let corner_pref: i32 = 2; // DWMWCP_ROUND
                    let _ = DwmSetWindowAttribute(
                        hwnd,
                        33,
                        &corner_pref as *const i32 as *const c_void,
                        size_of::<i32>() as u32,
                    );
                    thread_shared.hwnd.store(hwnd as isize, Ordering::SeqCst);

                    let mut msg: MSG = zeroed();
                    while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            })
            .ok();

        Self { shared }
    }

    /// Muestra o actualiza el mensaje flotante centrado en la parte superior del monitor indicado.
    pub fn show(&self, mon_x: i32, mon_y: i32, mon_w: u32, text: String, is_native: bool) {
        if let Ok(mut guard) = self.shared.payload.lock() {
            *guard = Some(ToastPayload {
                mon_x,
                mon_y,
                mon_w: mon_w as i32,
                text,
                is_native,
                started_at: Instant::now(),
            });
        }
        let raw_hwnd = self.shared.hwnd.load(Ordering::SeqCst);
        if raw_hwnd != 0 {
            unsafe {
                let _ = PostMessageW(raw_hwnd as HWND, WM_APP_SHOW_TOAST, 0, 0);
            }
        }
    }
}
