// Script de compilación: incrusta icono, metadatos PE y manifiesto estándar de Windows.
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "AspectShift");
        res.set(
            "FileDescription",
            "AspectShift — Gestor de Resoluciones y Relación de Aspecto",
        );
        res.set("CompanyName", "Alexander Simpertigue");
        res.set("OriginalFilename", "AspectShift.exe");
        res.set("InternalName", "AspectShift");
        res.set("LegalCopyright", "Copyright (c) 2026 Alexander Simpertigue");
        res.set_manifest(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity version="1.3.0.0" processorArchitecture="amd64" name="AspectShift" type="win32" />
  <description>Gestor de resoluciones y relacion de aspecto</description>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}" />
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>"#,
        );
        if let Err(err) = res.compile() {
            println!("cargo:warning=No se pudo compilar el recurso de Windows: {err}");
        }
    }
}
