#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod display;
mod hotkeys;
mod integration;
mod presets;
mod toast;
mod updater;
mod window_stretch;

use std::ffi::c_void;
use std::mem::size_of;

use app::AspectShiftApp;
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW;

static ICON_RGBA_64: &[u8] = include_bytes!("../assets/icon_rgba.bin");

/// Activa el modo oscuro nativo de Windows en la barra de título mediante DWM.
fn enable_dark_titlebar_by_title(title: &str) {
    let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), wide.as_ptr());
        if !hwnd.is_null() {
            let dark: i32 = 1;
            let _ = DwmSetWindowAttribute(
                hwnd,
                20,
                &dark as *const i32 as *const c_void,
                size_of::<i32>() as u32,
            );
            let _ = DwmSetWindowAttribute(
                hwnd,
                19,
                &dark as *const i32 as *const c_void,
                size_of::<i32>() as u32,
            );
        }
    }
}

fn main() -> eframe::Result<()> {
    // Modo de diagnóstico rápido para comprobar monitores detectados desde consola o pruebas.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--register-start-menu") {
        let _ = integration::register_in_start_menu();
        return Ok(());
    }
    if args.iter().any(|a| a == "--verify") {
        let monitors = display::enumerate_monitors();
        let windows = window_stretch::scan_candidate_windows();
        let report = serde_json::json!({
            "status": "ok",
            "monitors_count": monitors.len(),
            "monitors": monitors.iter().map(|m| serde_json::json!({
                "device_name": m.device_name,
                "friendly_name": m.friendly_name,
                "is_primary": m.is_primary,
                "current_mode": format!("{}x{}@{}Hz", m.current_mode.width, m.current_mode.height, m.current_mode.hz),
                "native_mode": format!("{}x{}@{}Hz", m.native_mode.width, m.native_mode.height, m.native_mode.hz),
                "supported_resolutions_count": m.supported_resolutions.len(),
                "supported_hz": m.all_supported_hz,
            })).collect::<Vec<_>>(),
            "detected_windows_count": windows.len(),
        });
        if let Ok(text) = serde_json::to_string_pretty(&report) {
            let _ = std::fs::write("verify_report.json", &text);
        }
        return Ok(());
    }

    let just_updated = args.iter().any(|a| a == "--updated");
    let icon_data = egui::IconData {
        rgba: ICON_RGBA_64.to_vec(),
        width: 64,
        height: 64,
    };

    let window_title = "AspectShift";
    let config = config::AppConfig::load();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(window_title)
        .with_app_id("AspectShift")
        .with_inner_size([520.0, 440.0])
        .with_min_inner_size([340.0, 340.0])
        .with_icon(std::sync::Arc::new(icon_data));
    if config.always_on_top {
        viewport = viewport.with_always_on_top();
    }

    let native_options = eframe::NativeOptions {
        viewport,
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        window_title,
        native_options,
        Box::new(move |cc| {
            enable_dark_titlebar_by_title(window_title);
            Ok(Box::new(AspectShiftApp::new(cc, ICON_RGBA_64, just_updated)))
        }),
    )
}
