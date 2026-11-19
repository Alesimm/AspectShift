use std::collections::{BTreeSet, HashSet};
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::time::Instant;

use windows_sys::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig, SetDisplayConfig,
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_HEADER,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE,
    DISPLAYCONFIG_MODE_INFO_TYPE_TARGET, DISPLAYCONFIG_PATH_INFO,
    DISPLAYCONFIG_SCALING_ASPECTRATIOCENTEREDMAX, DISPLAYCONFIG_SCALING_IDENTITY,
    DISPLAYCONFIG_SCALING_STRETCHED, DISPLAYCONFIG_SOURCE_DEVICE_NAME, QDC_ONLY_ACTIVE_PATHS,
    SDC_ALLOW_CHANGES, SDC_APPLY, SDC_NO_OPTIMIZATION, SDC_USE_SUPPLIED_DISPLAY_CONFIG,
};
use windows_sys::Win32::Graphics::Gdi::{
    ChangeDisplaySettingsExW, EnumDisplayDevicesW, EnumDisplaySettingsExW, CDS_FULLSCREEN,
    DEVMODEW, DISPLAY_DEVICEW, DISPLAY_DEVICE_ACTIVE, DISPLAY_DEVICE_PRIMARY_DEVICE,
    DISP_CHANGE_BADDUALVIEW, DISP_CHANGE_BADFLAGS, DISP_CHANGE_BADMODE, DISP_CHANGE_BADPARAM,
    DISP_CHANGE_FAILED, DISP_CHANGE_NOTUPDATED, DISP_CHANGE_RESTART, DISP_CHANGE_SUCCESSFUL,
    DM_DISPLAYFIXEDOUTPUT, DM_DISPLAYFREQUENCY, DM_PELSHEIGHT, DM_PELSWIDTH, DM_POSITION,
    ENUM_CURRENT_SETTINGS, ENUM_REGISTRY_SETTINGS,
};

const DMDFO_DEFAULT: u32 = 0;
const DMDFO_STRETCH: u32 = 1;

/// Modo de pantalla activo o nativo de un monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    pub hz: u32,
    pub bpp: u32,
    pub pos_x: i32,
    pub pos_y: i32,
}

impl Default for DisplayMode {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            hz: 60,
            bpp: 32,
            pos_x: 0,
            pos_y: 0,
        }
    }
}

/// Información recopilada de cada monitor conectado y activo.
#[derive(Debug, Clone)]
pub struct MonitorInfo {
    pub device_name: String,
    pub friendly_name: String,
    pub is_primary: bool,
    pub current_mode: DisplayMode,
    pub native_mode: DisplayMode,
    pub supported_resolutions: HashSet<(u32, u32)>,
    pub all_supported_hz: Vec<u32>,
}

impl MonitorInfo {
    pub fn display_label(&self, index: usize) -> String {
        let primary_tag = if self.is_primary { " [Principal]" } else { "" };
        format!("Pantalla {}{}", index + 1, primary_tag)
    }

    /// Comprueba si una coordenada de pantalla cae dentro de los límites de este monitor.
    pub fn contains_point(&self, x: i32, y: i32) -> bool {
        let m = self.current_mode;
        x >= m.pos_x
            && x < m.pos_x + m.width as i32
            && y >= m.pos_y
            && y < m.pos_y + m.height as i32
    }
}

/// Resultado de un cambio de resolución exitoso.
#[derive(Debug, Clone)]
pub struct DisplayChangeResult {
    pub applied_mode: DisplayMode,
    pub message: String,
}

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide_slice(slice: &[u16]) -> String {
    let end = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
    String::from_utf16_lossy(&slice[..end]).trim().to_string()
}

fn query_mode_internal(device_name: &str, mode_num: u32) -> Option<(DisplayMode, DEVMODEW)> {
    let dev_wide = to_wide_null(device_name);
    unsafe {
        let mut dm: DEVMODEW = zeroed();
        dm.dmSize = size_of::<DEVMODEW>() as u16;
        let ok = EnumDisplaySettingsExW(dev_wide.as_ptr(), mode_num, &mut dm, 0);
        if ok == 0 || dm.dmPelsWidth == 0 || dm.dmPelsHeight == 0 {
            return None;
        }
        let pos = dm.Anonymous1.Anonymous2.dmPosition;
        let mode = DisplayMode {
            width: dm.dmPelsWidth,
            height: dm.dmPelsHeight,
            hz: dm.dmDisplayFrequency.max(60),
            bpp: dm.dmBitsPerPel.max(32),
            pos_x: pos.x,
            pos_y: pos.y,
        };
        Some((mode, dm))
    }
}

/// Consulta el modo actual del monitor indicado.
pub fn query_current_mode(device_name: &str) -> Option<DisplayMode> {
    query_mode_internal(device_name, ENUM_CURRENT_SETTINGS).map(|(m, _)| m)
}

fn query_registry_mode(device_name: &str) -> Option<DisplayMode> {
    query_mode_internal(device_name, ENUM_REGISTRY_SETTINGS).map(|(m, _)| m)
}

/// Enumera todos los monitores activos, sus modos y tasas de refresco soportadas.
pub fn enumerate_monitors() -> Vec<MonitorInfo> {
    let mut monitors = Vec::new();
    let mut dev_index = 0u32;

    unsafe {
        loop {
            let mut dd: DISPLAY_DEVICEW = zeroed();
            dd.cb = size_of::<DISPLAY_DEVICEW>() as u32;

            let res = EnumDisplayDevicesW(null(), dev_index, &mut dd, 0);
            if res == 0 {
                break;
            }
            dev_index += 1;

            if (dd.StateFlags & DISPLAY_DEVICE_ACTIVE) == 0 {
                continue;
            }

            let device_name = from_wide_slice(&dd.DeviceName);
            let adapter_name = from_wide_slice(&dd.DeviceString);
            let is_primary = (dd.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE) != 0;

            let mut mon_dd: DISPLAY_DEVICEW = zeroed();
            mon_dd.cb = size_of::<DISPLAY_DEVICEW>() as u32;
            let dev_wide = to_wide_null(&device_name);
            let friendly_name = if EnumDisplayDevicesW(dev_wide.as_ptr(), 0, &mut mon_dd, 0) != 0 {
                let mon_str = from_wide_slice(&mon_dd.DeviceString);
                if !mon_str.is_empty() && mon_str != "Generic PnP Monitor" {
                    format!("{} ({})", mon_str, adapter_name)
                } else if !adapter_name.is_empty() {
                    adapter_name.clone()
                } else {
                    device_name.trim_start_matches(r"\\.\").to_string()
                }
            } else if !adapter_name.is_empty() {
                adapter_name.clone()
            } else {
                device_name.trim_start_matches(r"\\.\").to_string()
            };

            let current_mode = query_current_mode(&device_name).unwrap_or_default();
            let reg_mode = query_registry_mode(&device_name).unwrap_or(current_mode);

            let mut supported_resolutions = HashSet::new();
            let mut all_hz_set: BTreeSet<u32> = BTreeSet::new();
            let mut max_16_9: Option<DisplayMode> = None;

            let mut mode_idx = 0u32;
            loop {
                let mut dm: DEVMODEW = zeroed();
                dm.dmSize = size_of::<DEVMODEW>() as u16;
                if EnumDisplaySettingsExW(dev_wide.as_ptr(), mode_idx, &mut dm, 0) == 0 {
                    break;
                }
                mode_idx += 1;

                if dm.dmBitsPerPel < 24 || dm.dmPelsWidth < 640 || dm.dmPelsHeight < 480 {
                    continue;
                }

                let w = dm.dmPelsWidth;
                let h = dm.dmPelsHeight;
                let hz = dm.dmDisplayFrequency;

                supported_resolutions.insert((w, h));
                if hz > 1 {
                    all_hz_set.insert(hz);
                }

                // Guarda el modo 16:9 de mayor tamaño y frecuencia como candidato nativo
                let ratio_diff = ((w as f64 / h as f64) - (16.0 / 9.0)).abs();
                if ratio_diff < 0.02 {
                    let candidate = DisplayMode {
                        width: w,
                        height: h,
                        hz: hz.max(60),
                        bpp: dm.dmBitsPerPel,
                        pos_x: current_mode.pos_x,
                        pos_y: current_mode.pos_y,
                    };
                    match max_16_9 {
                        None => max_16_9 = Some(candidate),
                        Some(prev) => {
                            if (w * h > prev.width * prev.height)
                                || (w == prev.width && h == prev.height && hz > prev.hz)
                            {
                                max_16_9 = Some(candidate);
                            }
                        }
                    }
                }
            }

            let mut all_supported_hz: Vec<u32> = all_hz_set.into_iter().collect();
            all_supported_hz.sort_by(|a, b| b.cmp(a));
            if all_supported_hz.is_empty() {
                all_supported_hz.push(current_mode.hz.max(60));
            }

            let reg_ratio = reg_mode.width as f64 / reg_mode.height.max(1) as f64;
            let native_mode = if reg_ratio >= 1.55 {
                reg_mode
            } else {
                max_16_9.unwrap_or(reg_mode)
            };

            monitors.push(MonitorInfo {
                device_name,
                friendly_name,
                is_primary,
                current_mode,
                native_mode,
                supported_resolutions,
                all_supported_hz,
            });
        }
    }

    monitors.sort_by_key(|m| !m.is_primary);
    monitors
}

/// Cambia la resolución mediante la API moderna CCD congelando el TargetMode físico y las demás pantallas.
fn try_apply_resolution_ccd(
    device_name: &str,
    width: u32,
    height: u32,
    target_hz: Option<u32>,
    force_stretch_scaling: bool,
) -> Option<DisplayChangeResult> {
    let start = Instant::now();
    unsafe {
        let mut path_count = 0u32;
        let mut mode_count = 0u32;
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count) != 0
            || path_count == 0
            || mode_count == 0
        {
            return None;
        }

        let mut paths: Vec<DISPLAYCONFIG_PATH_INFO> = vec![zeroed(); path_count as usize];
        let mut modes: Vec<DISPLAYCONFIG_MODE_INFO> = vec![zeroed(); mode_count as usize];
        if QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            null_mut(),
        ) != 0
        {
            return None;
        }
        paths.truncate(path_count as usize);
        modes.truncate(mode_count as usize);

        // Localiza el path activo que corresponde al nombre GDI seleccionado (ej. \\.\DISPLAY1)
        let mut target_path_idx: Option<usize> = None;
        for (i, p) in paths.iter().enumerate() {
            let mut src_name: DISPLAYCONFIG_SOURCE_DEVICE_NAME = zeroed();
            src_name.header = DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                size: size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                adapterId: p.sourceInfo.adapterId,
                id: p.sourceInfo.id,
            };
            if DisplayConfigGetDeviceInfo(&mut src_name.header) == 0
                && from_wide_slice(&src_name.viewGdiDeviceName) == device_name
            {
                target_path_idx = Some(i);
                break;
            }
        }

        let path_idx = target_path_idx?;
        let src_idx = paths[path_idx].sourceInfo.Anonymous.modeInfoIdx as usize;
        let tgt_idx = paths[path_idx].targetInfo.Anonymous.modeInfoIdx as usize;
        if src_idx >= modes.len()
            || tgt_idx >= modes.len()
            || modes[src_idx].infoType != DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE
            || modes[tgt_idx].infoType != DISPLAYCONFIG_MODE_INFO_TYPE_TARGET
        {
            return None;
        }

        let tgt_info = modes[tgt_idx].Anonymous.targetMode.targetVideoSignalInfo;
        let active_w = tgt_info.activeSize.cx;
        let active_h = tgt_info.activeSize.cy;
        if width > active_w || height > active_h {
            return None;
        }

        // Si el usuario pidió cambiar explícitamente los Hz respecto a la señal física actual, delega a GDI
        if let Some(req_hz) = target_hz {
            let cur_hz = if tgt_info.vSyncFreq.Denominator > 0 {
                (tgt_info.vSyncFreq.Numerator as f64 / tgt_info.vSyncFreq.Denominator as f64)
                    .round() as u32
            } else {
                0
            };
            if req_hz > 1 && cur_hz > 1 && req_hz.abs_diff(cur_hz) > 1 {
                return None;
            }
        }

        // Cambia solo el SourceMode y el escalado manteniendo congelado el TargetMode físico (Hz y timings)
        modes[src_idx].Anonymous.sourceMode.width = width;
        modes[src_idx].Anonymous.sourceMode.height = height;
        paths[path_idx].targetInfo.scaling = if width == active_w && height == active_h {
            DISPLAYCONFIG_SCALING_IDENTITY
        } else if force_stretch_scaling {
            DISPLAYCONFIG_SCALING_STRETCHED
        } else {
            DISPLAYCONFIG_SCALING_ASPECTRATIOCENTEREDMAX
        };

        let mut rc = SetDisplayConfig(
            paths.len() as u32,
            paths.as_ptr(),
            modes.len() as u32,
            modes.as_ptr(),
            SDC_APPLY | SDC_USE_SUPPLIED_DISPLAY_CONFIG | SDC_NO_OPTIMIZATION,
        );

        if rc != 0 {
            rc = SetDisplayConfig(
                paths.len() as u32,
                paths.as_ptr(),
                modes.len() as u32,
                modes.as_ptr(),
                SDC_APPLY | SDC_USE_SUPPLIED_DISPLAY_CONFIG | SDC_ALLOW_CHANGES,
            );
        }

        if rc != 0 {
            return None;
        }

        let verified = query_current_mode(device_name)?;
        if verified.width != width || verified.height != height {
            return None;
        }

        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        Some(DisplayChangeResult {
            applied_mode: verified,
            message: format!(
                "{} × {} @ {} Hz ({:.1} ms)",
                verified.width, verified.height, verified.hz, elapsed_ms
            ),
        })
    }
}

/// Aplica una nueva resolución únicamente sobre el monitor seleccionado sin alterar otras pantallas.
pub fn apply_resolution(
    device_name: &str,
    width: u32,
    height: u32,
    target_hz: Option<u32>,
    force_stretch_scaling: bool,
) -> Result<DisplayChangeResult, String> {
    if let Some(res) =
        try_apply_resolution_ccd(device_name, width, height, target_hz, force_stretch_scaling)
    {
        return Ok(res);
    }

    let start = Instant::now();
    let dev_wide = to_wide_null(device_name);

    let (_, mut base_dm) = query_mode_internal(device_name, ENUM_CURRENT_SETTINGS)
        .ok_or_else(|| "No se pudo leer la configuración actual del monitor.".to_string())?;

    base_dm.dmSize = size_of::<DEVMODEW>() as u16;
    base_dm.dmPelsWidth = width;
    base_dm.dmPelsHeight = height;
    // Preserva la posición del monitor en el escritorio extendido junto con el nuevo tamaño
    base_dm.dmFields |= DM_PELSWIDTH | DM_PELSHEIGHT | DM_POSITION;

    if let Some(hz) = target_hz {
        if hz > 1 {
            base_dm.dmDisplayFrequency = hz;
            base_dm.dmFields |= DM_DISPLAYFREQUENCY;
        }
    }

    unsafe {
        let mut dm_attempt = base_dm;
        if force_stretch_scaling {
            dm_attempt.Anonymous1.Anonymous2.dmDisplayFixedOutput = DMDFO_STRETCH;
            dm_attempt.dmFields |= DM_DISPLAYFIXEDOUTPUT;
        } else {
            dm_attempt.Anonymous1.Anonymous2.dmDisplayFixedOutput = DMDFO_DEFAULT;
        }

        // Intento 1: Cambio limpio con flags = 0 para respetar el resto de monitores
        let mut code =
            ChangeDisplaySettingsExW(dev_wide.as_ptr(), &mut dm_attempt, null_mut(), 0, null());

        // Intento 2: Reintentar sin el flag DM_DISPLAYFIXEDOUTPUT si el controlador no lo admite
        if code != DISP_CHANGE_SUCCESSFUL && force_stretch_scaling {
            let mut dm_no_fixed = base_dm;
            dm_no_fixed.dmFields &= !DM_DISPLAYFIXEDOUTPUT;
            code = ChangeDisplaySettingsExW(
                dev_wide.as_ptr(),
                &mut dm_no_fixed,
                null_mut(),
                0,
                null(),
            );
        }

        // Intento 3: Dejar que el controlador elija la frecuencia si rechazó la solicitada
        if code == DISP_CHANGE_BADMODE && target_hz.is_some() {
            let mut dm_auto_hz = base_dm;
            dm_auto_hz.dmFields &= !(DM_DISPLAYFIXEDOUTPUT | DM_DISPLAYFREQUENCY);
            let retry_code = ChangeDisplaySettingsExW(
                dev_wide.as_ptr(),
                &mut dm_auto_hz,
                null_mut(),
                0,
                null(),
            );
            if retry_code == DISP_CHANGE_SUCCESSFUL {
                code = retry_code;
            }
        }

        // Intento 4: Respaldo con CDS_FULLSCREEN si el driver exige modo temporal explícito
        if code != DISP_CHANGE_SUCCESSFUL {
            let mut dm_fs = base_dm;
            dm_fs.dmFields &= !DM_DISPLAYFIXEDOUTPUT;
            let retry_code = ChangeDisplaySettingsExW(
                dev_wide.as_ptr(),
                &mut dm_fs,
                null_mut(),
                CDS_FULLSCREEN,
                null(),
            );
            if retry_code == DISP_CHANGE_SUCCESSFUL {
                code = retry_code;
            }
        }

        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

        if code == DISP_CHANGE_SUCCESSFUL || code == DISP_CHANGE_RESTART {
            let verified = query_current_mode(device_name).unwrap_or(DisplayMode {
                width,
                height,
                hz: target_hz.unwrap_or(base_dm.dmDisplayFrequency),
                bpp: base_dm.dmBitsPerPel,
                pos_x: base_dm.Anonymous1.Anonymous2.dmPosition.x,
                pos_y: base_dm.Anonymous1.Anonymous2.dmPosition.y,
            });
            let msg = format!(
                "{} × {} @ {} Hz ({:.1} ms)",
                verified.width, verified.height, verified.hz, elapsed_ms
            );
            Ok(DisplayChangeResult {
                applied_mode: verified,
                message: msg,
            })
        } else {
            Err(explain_disp_change_error(code, width, height))
        }
    }
}

/// Restaura la resolución nativa guardada en el registro para el monitor indicado.
pub fn restore_native(
    device_name: &str,
    native_fallback: Option<DisplayMode>,
) -> Result<DisplayChangeResult, String> {
    let start = Instant::now();

    if let Some(native) = native_fallback {
        if let Some(mut res) = try_apply_resolution_ccd(
            device_name,
            native.width,
            native.height,
            Some(native.hz),
            false,
        ) {
            res.message = format!(
                "Nativa {} × {} @ {} Hz ({:.1} ms)",
                res.applied_mode.width,
                res.applied_mode.height,
                res.applied_mode.hz,
                start.elapsed().as_secs_f64() * 1000.0
            );
            return Ok(res);
        }
    }

    let dev_wide = to_wide_null(device_name);

    unsafe {
        let code = ChangeDisplaySettingsExW(dev_wide.as_ptr(), null(), null_mut(), 0, null_mut());
        let after = query_current_mode(device_name);

        if let (Some(curr), Some(native)) = (after, native_fallback) {
            if curr.width != native.width || curr.height != native.height {
                if let Ok(res) = apply_resolution(
                    device_name,
                    native.width,
                    native.height,
                    Some(native.hz),
                    false,
                ) {
                    return Ok(DisplayChangeResult {
                        applied_mode: res.applied_mode,
                        message: format!(
                            "Nativa {} × {} @ {} Hz ({:.1} ms)",
                            res.applied_mode.width,
                            res.applied_mode.height,
                            res.applied_mode.hz,
                            start.elapsed().as_secs_f64() * 1000.0
                        ),
                    });
                }
            }
        }

        if code == DISP_CHANGE_SUCCESSFUL {
            let verified = after.or(native_fallback).unwrap_or_default();
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            Ok(DisplayChangeResult {
                applied_mode: verified,
                message: format!(
                    "Nativa {} × {} @ {} Hz ({:.1} ms)",
                    verified.width, verified.height, verified.hz, elapsed_ms
                ),
            })
        } else if let Some(native) = native_fallback {
            apply_resolution(
                device_name,
                native.width,
                native.height,
                Some(native.hz),
                false,
            )
        } else {
            apply_resolution(device_name, 1920, 1080, None, false)
        }
    }
}

fn explain_disp_change_error(code: i32, width: u32, height: u32) -> String {
    match code {
        DISP_CHANGE_BADMODE => format!(
            "El driver GPU no tiene registrada {}×{} (código -2). Agrégala como resolución personalizada en el panel gráfico.",
            width, height
        ),
        DISP_CHANGE_FAILED => format!(
            "El driver rechazó {}×{} (código -1). Comprueba la frecuencia (Hz).",
            width, height
        ),
        DISP_CHANGE_BADFLAGS => "Flags de visualización inválidos (código -4).".to_string(),
        DISP_CHANGE_BADPARAM => "Parámetros de resolución fuera de rango (código -5).".to_string(),
        DISP_CHANGE_NOTUPDATED => {
            "No se pudo actualizar el perfil de pantalla (código -3).".to_string()
        }
        DISP_CHANGE_BADDUALVIEW => {
            "Configuración incompatible con el modo multipantalla (código -6).".to_string()
        }
        other => format!("Error al aplicar {}×{} (código {}).", width, height, other),
    }
}
