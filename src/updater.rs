use std::fs;
use std::mem::zeroed;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::ptr::{null, null_mut};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use serde::Deserialize;
use windows_sys::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpCrackUrl, WinHttpOpen, WinHttpOpenRequest,
    WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts, URL_COMPONENTS,
    WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_OPTION_REDIRECT_POLICY,
    WINHTTP_OPTION_REDIRECT_POLICY_ALWAYS, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

const GITHUB_LATEST_URL: &str = "https://api.github.com/repos/Alesimm/AspectShift/releases/latest";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub version_tag: String,
    pub download_url: String,
    pub expected_size: u64,
}

#[derive(Debug, Clone)]
pub enum UpdateEvent {
    CheckResult {
        manual: bool,
        result: Result<Option<ReleaseInfo>, String>,
    },
    DownloadFinished(Result<PathBuf, String>),
}

#[derive(Deserialize)]
struct GitHubReleaseApi {
    tag_name: String,
    assets: Vec<GitHubAssetApi>,
}

#[derive(Deserialize)]
struct GitHubAssetApi {
    name: String,
    size: u64,
    browser_download_url: String,
}

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn parse_semver(tag: &str) -> Option<(u32, u32, u32)> {
    let clean = tag.trim().trim_start_matches(['v', 'V']);
    let clean = clean.split('-').next().unwrap_or(clean);
    let mut parts = clean.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next().unwrap_or("0").parse::<u32>().ok()?;
    let patch = parts.next().unwrap_or("0").parse::<u32>().ok()?;
    Some((major, minor, patch))
}

fn is_newer_version(remote_tag: &str, current_version: &str) -> bool {
    match (parse_semver(remote_tag), parse_semver(current_version)) {
        (Some(r), Some(c)) => r > c,
        _ => false,
    }
}

/// Realiza una petición HTTPS GET nativa mediante WinHTTP (usa TLS del sistema y sigue redirecciones).
fn winhttp_get(url: &str) -> Result<Vec<u8>, String> {
    let user_agent = to_wide_null(&format!("AspectShift/{}", CURRENT_VERSION));
    let url_wide: Vec<u16> = url.encode_utf16().collect();

    unsafe {
        let mut host_buf = [0u16; 256];
        let mut path_buf = [0u16; 2048];
        let mut comps: URL_COMPONENTS = zeroed();
        comps.dwStructSize = std::mem::size_of::<URL_COMPONENTS>() as u32;
        comps.lpszHostName = host_buf.as_mut_ptr();
        comps.dwHostNameLength = host_buf.len() as u32;
        comps.lpszUrlPath = path_buf.as_mut_ptr();
        comps.dwUrlPathLength = path_buf.len() as u32;

        if WinHttpCrackUrl(url_wide.as_ptr(), url_wide.len() as u32, 0, &mut comps) == 0 {
            return Err("URL de actualización inválida.".to_string());
        }

        let host_len = comps.dwHostNameLength as usize;
        let path_len = comps.dwUrlPathLength as usize;
        let mut host_w = host_buf[..host_len].to_vec();
        host_w.push(0);
        let mut path_w = path_buf[..path_len].to_vec();
        path_w.push(0);

        let session = WinHttpOpen(
            user_agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            null(),
            null(),
            0,
        );
        if session.is_null() {
            return Err("No se pudo iniciar la sesión de red.".to_string());
        }

        let _ = WinHttpSetTimeouts(session, 5000, 7000, 10000, 20000);
        let redirect_policy = WINHTTP_OPTION_REDIRECT_POLICY_ALWAYS;
        let _ = WinHttpSetOption(
            session,
            WINHTTP_OPTION_REDIRECT_POLICY,
            &redirect_policy as *const u32 as *const _,
            std::mem::size_of::<u32>() as u32,
        );

        let connect = WinHttpConnect(session, host_w.as_ptr(), comps.nPort, 0);
        if connect.is_null() {
            WinHttpCloseHandle(session);
            return Err("No se pudo conectar con GitHub.".to_string());
        }

        let verb = to_wide_null("GET");
        let flags = if comps.nPort == 443 {
            WINHTTP_FLAG_SECURE
        } else {
            0
        };
        let request = WinHttpOpenRequest(
            connect,
            verb.as_ptr(),
            path_w.as_ptr(),
            null(),
            null(),
            null_mut(),
            flags,
        );
        if request.is_null() {
            WinHttpCloseHandle(connect);
            WinHttpCloseHandle(session);
            return Err("No se pudo crear la solicitud HTTPS.".to_string());
        }

        let sent = WinHttpSendRequest(request, null(), 0, null(), 0, 0, 0);
        if sent == 0 || WinHttpReceiveResponse(request, null_mut()) == 0 {
            WinHttpCloseHandle(request);
            WinHttpCloseHandle(connect);
            WinHttpCloseHandle(session);
            return Err("Sin respuesta del servidor de GitHub.".to_string());
        }

        let mut status_code: u32 = 0;
        let mut status_size = std::mem::size_of::<u32>() as u32;
        let _ = WinHttpQueryHeaders(
            request,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            null(),
            &mut status_code as *mut u32 as *mut _,
            &mut status_size,
            null_mut(),
        );
        if status_code != 200 {
            WinHttpCloseHandle(request);
            WinHttpCloseHandle(connect);
            WinHttpCloseHandle(session);
            return Err(format!("GitHub devolvió código HTTP {}.", status_code));
        }

        let mut body = Vec::new();
        loop {
            let mut avail: u32 = 0;
            if WinHttpQueryDataAvailable(request, &mut avail) == 0 {
                break;
            }
            if avail == 0 {
                break;
            }
            let mut chunk = vec![0u8; avail as usize];
            let mut downloaded: u32 = 0;
            if WinHttpReadData(
                request,
                chunk.as_mut_ptr() as *mut _,
                avail,
                &mut downloaded,
            ) == 0
                || downloaded == 0
            {
                break;
            }
            body.extend_from_slice(&chunk[..downloaded as usize]);
        }

        WinHttpCloseHandle(request);
        WinHttpCloseHandle(connect);
        WinHttpCloseHandle(session);

        Ok(body)
    }
}

/// Gestiona comprobaciones y descargas en segundo plano sin bloquear la interfaz.
pub struct UpdateManager {
    tx: mpsc::Sender<UpdateEvent>,
    rx: Receiver<UpdateEvent>,
}

impl UpdateManager {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { tx, rx }
    }

    pub fn poll(&self) -> Option<UpdateEvent> {
        self.rx.try_recv().ok()
    }

    /// Consulta en segundo plano la última versión publicada en GitHub Releases.
    pub fn check_for_updates(&self, ctx: egui::Context, manual: bool) {
        let tx = self.tx.clone();
        thread::Builder::new()
            .name("aspectshift-update-check".to_string())
            .spawn(move || {
                let res = (|| -> Result<Option<ReleaseInfo>, String> {
                    let data = winhttp_get(GITHUB_LATEST_URL)?;
                    let release: GitHubReleaseApi = serde_json::from_slice(&data)
                        .map_err(|_| "No se pudo leer la respuesta de versión.".to_string())?;

                    if !is_newer_version(&release.tag_name, CURRENT_VERSION) {
                        return Ok(None);
                    }

                    let asset = release
                        .assets
                        .into_iter()
                        .find(|a| a.name.eq_ignore_ascii_case("AspectShift.exe"))
                        .ok_or_else(|| {
                            "El nuevo lanzamiento no incluye ejecutable.".to_string()
                        })?;

                    Ok(Some(ReleaseInfo {
                        version_tag: release.tag_name,
                        download_url: asset.browser_download_url,
                        expected_size: asset.size,
                    }))
                })();

                let _ = tx.send(UpdateEvent::CheckResult {
                    manual,
                    result: res,
                });
                ctx.request_repaint();
            })
            .ok();
    }

    /// Descarga el nuevo ejecutable en `%TEMP%` y verifica que sea un binario PE válido.
    pub fn download_release(&self, ctx: egui::Context, info: ReleaseInfo) {
        let tx = self.tx.clone();
        thread::Builder::new()
            .name("aspectshift-update-dl".to_string())
            .spawn(move || {
                let res = (|| -> Result<PathBuf, String> {
                    let bytes = winhttp_get(&info.download_url)?;
                    if bytes.len() < 512_000 || !bytes.starts_with(b"MZ") {
                        return Err("El archivo descargado no es un ejecutable válido.".to_string());
                    }
                    if info.expected_size > 0 && bytes.len() as u64 != info.expected_size {
                        return Err("La descarga quedó incompleta.".to_string());
                    }

                    let temp_path = std::env::temp_dir().join("AspectShift_new.exe");
                    fs::write(&temp_path, &bytes)
                        .map_err(|e| format!("No se pudo guardar la actualización: {}", e))?;
                    Ok(temp_path)
                })();

                let _ = tx.send(UpdateEvent::DownloadFinished(res));
                ctx.request_repaint();
            })
            .ok();
    }
}

/// Lanza un script temporal oculto que reemplaza el `.exe` actual tras cerrarse y vuelve a abrirlo.
pub fn apply_downloaded_update(temp_exe: &PathBuf) -> Result<(), String> {
    let current_exe = std::env::current_exe()
        .map_err(|_| "No se pudo determinar la ruta del ejecutable actual.".to_string())?;

    let script_path = std::env::temp_dir().join("aspectshift_updater.cmd");
    let src = temp_exe.to_string_lossy();
    let dst = current_exe.to_string_lossy();

    let bat = format!(
        "@echo off\r\n\
         set \"SRC={src}\"\r\n\
         set \"DST={dst}\"\r\n\
         for /L %%i in (1,1,15) do (\r\n\
           ping 127.0.0.1 -n 2 > nul\r\n\
           copy /Y \"%SRC%\" \"%DST%\" > nul 2>&1\r\n\
           if not errorlevel 1 goto done\r\n\
         )\r\n\
         :done\r\n\
         del /Q \"%SRC%\" > nul 2>&1\r\n\
         start \"\" \"%DST%\"\r\n\
         del \"%~f0\" > nul 2>&1\r\n"
    );

    fs::write(&script_path, bat)
        .map_err(|e| format!("No se pudo preparar el actualizador: {}", e))?;

    Command::new("cmd")
        .args(["/C", &script_path.to_string_lossy()])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("No se pudo iniciar el proceso de actualización: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_comparison_works() {
        assert!(is_newer_version("v1.2.1", "1.2.0"));
        assert!(is_newer_version("v2.0.0", "1.2.0"));
        assert!(!is_newer_version("v1.2.0", "1.2.0"));
        assert!(!is_newer_version("v1.1.0", "1.2.0"));
    }

    #[test]
    fn winhttp_fetches_github_latest_release() {
        let data = winhttp_get(GITHUB_LATEST_URL).expect("WinHTTP GET should succeed");
        let parsed: GitHubReleaseApi =
            serde_json::from_slice(&data).expect("GitHub API JSON should parse");
        assert!(!parsed.tag_name.is_empty());
        assert!(parsed
            .assets
            .iter()
            .any(|a| a.name.eq_ignore_ascii_case("AspectShift.exe")));
    }
}
