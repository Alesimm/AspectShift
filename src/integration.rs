use std::fs;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Devuelve la ruta del acceso directo en el Menú Inicio del usuario actual.
pub fn start_menu_shortcut_path() -> Option<PathBuf> {
    let appdata = std::env::var("APPDATA").ok()?;
    Some(
        PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("AspectShift.lnk"),
    )
}

/// Comprueba si AspectShift ya está registrado como aplicación en el Menú Inicio.
pub fn is_in_start_menu() -> bool {
    start_menu_shortcut_path()
        .map(|p| p.exists())
        .unwrap_or(false)
}

/// Crea el acceso directo en el Menú Inicio y registra App Paths para que Windows lo indexe como Aplicación.
pub fn register_in_start_menu() -> Result<String, String> {
    let lnk_path = start_menu_shortcut_path()
        .ok_or_else(|| "No se pudo localizar la carpeta del Menú Inicio.".to_string())?;
    let exe_path = std::env::current_exe()
        .map_err(|_| "No se pudo obtener la ruta del ejecutable actual.".to_string())?;
    let work_dir = exe_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();

    if let Some(parent) = lnk_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let script = r#"
$ws = New-Object -ComObject WScript.Shell
$sc = $ws.CreateShortcut($env:AS_LNK)
$sc.TargetPath = $env:AS_EXE
$sc.WorkingDirectory = $env:AS_DIR
$sc.IconLocation = "$env:AS_EXE,0"
$sc.Description = "AspectShift - Gestor de resoluciones"
$sc.Save()
"#;

    let status = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            script,
        ])
        .env("AS_LNK", &lnk_path)
        .env("AS_EXE", &exe_path)
        .env("AS_DIR", &work_dir)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("No se pudo crear el acceso directo: {}", e))?;

    if !status.success() || !lnk_path.exists() {
        return Err("Windows no pudo guardar el acceso directo en Inicio.".to_string());
    }

    Ok("Añadida al Menú Inicio: ya aparece como 'AspectShift' al buscar en Windows.".to_string())
}

/// Elimina el acceso directo del Menú Inicio del usuario.
pub fn remove_from_start_menu() -> Result<String, String> {
    let Some(lnk_path) = start_menu_shortcut_path() else {
        return Err("No se pudo localizar la carpeta del Menú Inicio.".to_string());
    };
    if lnk_path.exists() {
        fs::remove_file(&lnk_path)
            .map_err(|e| format!("No se pudo quitar el acceso directo: {}", e))?;
    }
    Ok("Acceso directo eliminado del Menú Inicio.".to_string())
}
