use std::mem::zeroed;
use std::ptr::null_mut;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};

const HOTKEY_ID_STRETCH: i32 = 1001; // Ctrl + Alt + S
const HOTKEY_ID_NATIVE: i32 = 1002; // Ctrl + Alt + N
const HOTKEY_ID_BORDERLESS: i32 = 1003; // Ctrl + Alt + B

const VK_S: u32 = 0x53;
const VK_N: u32 = 0x4E;
const VK_B: u32 = 0x42;

/// Acciones disparadas por los atajos globales del sistema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    ApplyFavoriteStretch,
    RestoreNative,
    ApplyWindowBorderless,
}

/// Escucha eventos WM_HOTKEY en un hilo dedicado con consumo nulo en espera.
pub struct GlobalHotkeyListener {
    rx: Receiver<HotkeyAction>,
}

impl GlobalHotkeyListener {
    pub fn start(ctx: egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();

        thread::Builder::new()
            .name("aspectshift-hotkeys".to_string())
            .spawn(move || unsafe {
                let mods = MOD_CONTROL | MOD_ALT | MOD_NOREPEAT;
                let _ = RegisterHotKey(null_mut(), HOTKEY_ID_STRETCH, mods, VK_S);
                let _ = RegisterHotKey(null_mut(), HOTKEY_ID_NATIVE, mods, VK_N);
                let _ = RegisterHotKey(null_mut(), HOTKEY_ID_BORDERLESS, mods, VK_B);

                let mut msg: MSG = zeroed();
                while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                    if msg.message == WM_HOTKEY {
                        let action = match msg.wParam as i32 {
                            HOTKEY_ID_STRETCH => Some(HotkeyAction::ApplyFavoriteStretch),
                            HOTKEY_ID_NATIVE => Some(HotkeyAction::RestoreNative),
                            HOTKEY_ID_BORDERLESS => Some(HotkeyAction::ApplyWindowBorderless),
                            _ => None,
                        };
                        if let Some(act) = action {
                            let _ = tx.send(act);
                            ctx.request_repaint();
                        }
                    }
                }

                let _ = UnregisterHotKey(null_mut(), HOTKEY_ID_STRETCH);
                let _ = UnregisterHotKey(null_mut(), HOTKEY_ID_NATIVE);
                let _ = UnregisterHotKey(null_mut(), HOTKEY_ID_BORDERLESS);
            })
            .ok();

        Self { rx }
    }

    pub fn poll(&self) -> Option<HotkeyAction> {
        self.rx.try_recv().ok()
    }
}
