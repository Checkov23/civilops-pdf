//! Windows only: embed the app icon and version info (VERSIONINFO) into `printcraft.exe`, so it
//! shows in Explorer, the taskbar, the Start menu and Alt-Tab.
//!
//! On every other target this does nothing. A missing resource compiler is a warning, so a
//! cross-compile from macOS or Linux still links, unless `PRINTCRAFT_REQUIRE_WINRES=1` turns it
//! into an error (for release builds).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/civilops/civilops-pdf.ico");
    println!("cargo:rerun-if-env-changed=PRINTCRAFT_REQUIRE_WINRES");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/civilops/civilops-pdf.ico")
        .set("ProductName", "CivilOps PDF")
        .set("FileDescription", "CivilOps PDF")
        .set("CompanyName", "CivilOps")
        .set("LegalCopyright", "Copyright (c) 2026 CivilOps")
        .set("OriginalFilename", "civilops-pdf.exe")
        .set("InternalName", "civilops-pdf");
    if let Err(e) = res.compile() {
        if std::env::var_os("PRINTCRAFT_REQUIRE_WINRES").is_some() {
            println!("cargo::error=embedding Windows resources failed: {e}");
            return;
        }
        println!("cargo:warning=printcraft.exe built without icon/version resources: {e}");
    }
}
