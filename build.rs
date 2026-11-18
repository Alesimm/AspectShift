// Script de compilación: incrusta el icono y metadatos PE en el binario de Windows.
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
        res.set("LegalCopyright", "Copyright (c) 2026 Alexander Simpertigue");
        if let Err(err) = res.compile() {
            println!("cargo:warning=No se pudo compilar el recurso de icono: {err}");
        }
    }
}
