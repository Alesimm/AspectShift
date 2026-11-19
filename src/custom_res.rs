use std::ffi::{c_void, CString};
use std::mem::{size_of, transmute, zeroed};
use std::os::windows::process::CommandExt;
use std::process::Command;

use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// Identificadores internos de consulta en nvapi64.dll (nvapi_QueryInterface)
const ID_NVAPI_INITIALIZE: u32 = 0x0150_E828;
const ID_NVAPI_UNLOAD: u32 = 0xD22B_DD7E;
const ID_DISP_GET_DISPLAY_ID_BY_NAME: u32 = 0xAE45_7190;
const ID_DISP_GET_TIMING: u32 = 0x1751_67E9;
const ID_DISP_TRY_CUSTOM_DISPLAY: u32 = 0x1F7D_B630;
const ID_DISP_SAVE_CUSTOM_DISPLAY: u32 = 0x4988_2876;
const ID_DISP_REVERT_CUSTOM_TRIAL: u32 = 0xCBBD_40F0;

const NV_FORMAT_A8R8G8B8: i32 = 21;

/// Fabricante detectado de la tarjeta gráfica principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Unknown,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NvViewportF {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NvTimingExt {
    flag: u32,
    rr: u16,
    rrx1k: u32,
    aspect: u32,
    rep: u16,
    status: u32,
    name: [u8; 40],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NvTiming {
    h_visible: u16,
    h_border: u16,
    h_front_porch: u16,
    h_sync_width: u16,
    h_total: u16,
    h_sync_pol: u8,
    v_visible: u16,
    v_border: u16,
    v_front_porch: u16,
    v_sync_width: u16,
    v_total: u16,
    v_sync_pol: u8,
    interlaced: u16,
    pclk: u32,
    etc: NvTimingExt,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NvCustomDisplay {
    version: u32,
    width: u32,
    height: u32,
    depth: u32,
    color_format: i32,
    src_partition: NvViewportF,
    x_ratio: f32,
    y_ratio: f32,
    timing: NvTiming,
    hw_mode_set_only: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NvTimingInput {
    version: u32,
    width: u32,
    height: u32,
    rr: f32,
    flag: [u32; 3],
    timing_type: i32,
}

type FnQueryInterface = unsafe extern "C" fn(u32) -> *const c_void;
type FnInit = unsafe extern "C" fn() -> i32;
type FnUnload = unsafe extern "C" fn() -> i32;
type FnGetDisplayId = unsafe extern "C" fn(*const i8, *mut u32) -> i32;
type FnGetTiming = unsafe extern "C" fn(u32, *const NvTimingInput, *mut NvTiming) -> i32;
type FnTryCustomDisplay = unsafe extern "C" fn(*mut u32, u32, *mut NvCustomDisplay) -> i32;
type FnSaveCustomDisplay = unsafe extern "C" fn(*mut u32, u32, u32, u32) -> i32;
type FnRevertTrial = unsafe extern "C" fn(*mut u32, u32) -> i32;

struct NvApiSession {
    module: HMODULE,
    query: FnQueryInterface,
    initialized: bool,
}

impl NvApiSession {
    unsafe fn open() -> Result<Self, String> {
        let dll_wide: Vec<u16> = "nvapi64.dll"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let module = LoadLibraryW(dll_wide.as_ptr());
        if module.is_null() {
            return Err("No se encontró nvapi64.dll en el sistema.".to_string());
        }

        let proc = GetProcAddress(module, b"nvapi_QueryInterface\0".as_ptr());
        let Some(func_ptr) = proc else {
            FreeLibrary(module);
            return Err("No se pudo localizar nvapi_QueryInterface.".to_string());
        };

        let query: FnQueryInterface = transmute(func_ptr);
        let init_ptr = query(ID_NVAPI_INITIALIZE);
        if init_ptr.is_null() {
            FreeLibrary(module);
            return Err("NVAPI no está disponible en este controlador.".to_string());
        }

        let init: FnInit = transmute(init_ptr);
        let rc = init();
        if rc != 0 {
            FreeLibrary(module);
            return Err("No se pudo inicializar el controlador NVIDIA.".to_string());
        }

        Ok(Self {
            module,
            query,
            initialized: true,
        })
    }

    unsafe fn resolve(&self, id: u32) -> Option<*const c_void> {
        let ptr = (self.query)(id);
        if ptr.is_null() {
            None
        } else {
            Some(ptr)
        }
    }
}

impl Drop for NvApiSession {
    fn drop(&mut self) {
        unsafe {
            if self.initialized {
                let unload_ptr = (self.query)(ID_NVAPI_UNLOAD);
                if !unload_ptr.is_null() {
                    let unload: FnUnload = transmute(unload_ptr);
                    let _ = unload();
                }
            }
            if !self.module.is_null() {
                let _ = FreeLibrary(self.module);
            }
        }
    }
}

/// Detecta dinámicamente si el equipo usa una GPU NVIDIA (vía NVAPI) o de otro fabricante.
pub fn detect_gpu_vendor(adapter_hint: &str) -> GpuVendor {
    unsafe {
        if NvApiSession::open().is_ok() {
            return GpuVendor::Nvidia;
        }
    }

    let lower = adapter_hint.to_lowercase();
    if lower.contains("nvidia") || lower.contains("geforce") || lower.contains("rtx") {
        GpuVendor::Nvidia
    } else if lower.contains("amd") || lower.contains("radeon") {
        GpuVendor::Amd
    } else if lower.contains("intel") {
        GpuVendor::Intel
    } else {
        GpuVendor::Unknown
    }
}

/// Crea y guarda una resolución personalizada directamente en el controlador NVIDIA.
pub fn create_nvidia_custom_resolution(
    device_name: &str,
    width: u32,
    height: u32,
    native_w: u32,
    native_h: u32,
    hz: u32,
) -> Result<String, String> {
    let dev_cstr =
        CString::new(device_name).map_err(|_| "Nombre de monitor inválido.".to_string())?;

    unsafe {
        let session = NvApiSession::open()?;

        let get_disp_id: FnGetDisplayId = transmute(
            session
                .resolve(ID_DISP_GET_DISPLAY_ID_BY_NAME)
                .ok_or_else(|| "Función NVAPI de identificador no disponible.".to_string())?,
        );
        let get_timing: FnGetTiming = transmute(
            session
                .resolve(ID_DISP_GET_TIMING)
                .ok_or_else(|| "Función NVAPI de timings no disponible.".to_string())?,
        );
        let try_custom: FnTryCustomDisplay = transmute(
            session
                .resolve(ID_DISP_TRY_CUSTOM_DISPLAY)
                .ok_or_else(|| "Función NVAPI de prueba no disponible.".to_string())?,
        );
        let save_custom: FnSaveCustomDisplay = transmute(
            session
                .resolve(ID_DISP_SAVE_CUSTOM_DISPLAY)
                .ok_or_else(|| "Función NVAPI de guardado no disponible.".to_string())?,
        );
        let revert_trial: Option<FnRevertTrial> =
            session.resolve(ID_DISP_REVERT_CUSTOM_TRIAL).map(|p| transmute(p));

        let mut disp_id: u32 = 0;
        let rc_id = get_disp_id(dev_cstr.as_ptr(), &mut disp_id);
        if rc_id != 0 || disp_id == 0 {
            return Err("No se encontró el monitor seleccionado en NVIDIA.".to_string());
        }

        // Obtiene los timings nativos del monitor para escalar por GPU sin cambiar la señal física
        let mut timing: NvTiming = zeroed();
        let mut t_in = NvTimingInput {
            version: (size_of::<NvTimingInput>() as u32) | (1 << 16),
            width: native_w.max(width),
            height: native_h.max(height),
            rr: hz.max(60) as f32,
            flag: [0; 3],
            timing_type: 0,
        };

        let mut rc_timing = get_timing(disp_id, &t_in, &mut timing);
        if rc_timing != 0 {
            t_in.width = width;
            t_in.height = height;
            rc_timing = get_timing(disp_id, &t_in, &mut timing);
        }
        if rc_timing != 0 {
            return Err(format!(
                "No se pudieron calcular los timings para {}×{} @ {} Hz.",
                width, height, hz
            ));
        }

        let mut custom = NvCustomDisplay {
            version: (size_of::<NvCustomDisplay>() as u32) | (1 << 16),
            width,
            height,
            depth: 32,
            color_format: NV_FORMAT_A8R8G8B8,
            src_partition: NvViewportF {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            },
            x_ratio: 1.0,
            y_ratio: 1.0,
            timing,
            hw_mode_set_only: 0,
        };

        let rc_try = try_custom(&mut disp_id, 1, &mut custom);
        if rc_try != 0 {
            if let Some(rev) = revert_trial {
                let _ = rev(&mut disp_id, 1);
            }
            return Err(explain_nvapi_error(rc_try, width, height, hz));
        }

        let rc_save = save_custom(&mut disp_id, 1, 1, 1);
        if rc_save != 0 {
            if let Some(rev) = revert_trial {
                let _ = rev(&mut disp_id, 1);
            }
            return Err(explain_nvapi_error(rc_save, width, height, hz));
        }

        Ok(format!(
            "Resolución {}×{} @ {} Hz creada en NVIDIA",
            width, height, hz
        ))
    }
}

fn explain_nvapi_error(code: i32, width: u32, height: u32, hz: u32) -> String {
    match code {
        -129 | -7 | -5 => format!(
            "NVIDIA rechazó crear {}×{} @ {} Hz (puede requerir abrir AspectShift como Administrador o usar el Panel NVIDIA).",
            width, height, hz
        ),
        -10 => format!(
            "{}×{} @ {} Hz no es compatible con este monitor según el driver.",
            width, height, hz
        ),
        _ => format!(
            "No se pudo guardar {}×{} en NVIDIA (código {}). Prueba abrir como Administrador o usar el Panel NVIDIA.",
            width, height, code
        ),
    }
}

/// Abre el panel de control gráfico correspondiente (o Configuración de Pantalla como respaldo).
pub fn open_gpu_control_panel(vendor: GpuVendor) -> Result<String, String> {
    let launched = match vendor {
        GpuVendor::Nvidia => Command::new("nvcplui.exe")
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .or_else(|_| {
                Command::new("explorer.exe")
                    .arg(r"shell:AppsFolder\NVIDIACorp.NVIDIAControlPanel_56jybvy8sckqj!NVIDIACorp.NVIDIAControlPanel")
                    .creation_flags(CREATE_NO_WINDOW)
                    .spawn()
            }),
        GpuVendor::Amd => Command::new("explorer.exe")
            .arg(r"shell:AppsFolder\AdvancedMicroDevicesInc-2.AMDRadeonSoftware_0a9344xs7nr4m!AMDRadeonSoftware")
            .creation_flags(CREATE_NO_WINDOW)
            .spawn(),
        GpuVendor::Intel | GpuVendor::Unknown => Command::new("cmd")
            .args(["/C", "start", "", "ms-settings:display"])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn(),
    };

    if launched.is_ok() {
        return Ok("Abriendo panel de pantalla de tu GPU...".to_string());
    }

    Command::new("cmd")
        .args(["/C", "start", "", "ms-settings:display"])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|_| "No se pudo abrir la configuración de pantalla.".to_string())?;

    Ok("Abriendo configuración de pantalla de Windows...".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvapi_struct_sizes_match_driver_abi() {
        assert_eq!(size_of::<NvTimingExt>(), 64);
        assert_eq!(size_of::<NvTiming>(), 96);
        assert_eq!(size_of::<NvCustomDisplay>(), 144);
        assert_eq!(size_of::<NvTimingInput>(), 32);
    }

    #[test]
    fn gpu_vendor_fallback_parses_hint() {
        let vendor = detect_gpu_vendor("NVIDIA GeForce RTX 4060");
        assert_eq!(vendor, GpuVendor::Nvidia);
    }
}
