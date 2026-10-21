//! Windows only: embed the app icon, version info (VERSIONINFO) and the application manifest
//! (Per-Monitor V2 DPI awareness, `src/windows_manifest.rs`) into `civilops-pdf.exe`, so it shows
//! in Explorer, the taskbar, the Start menu and Alt-Tab.
//!
//! On every other target this does nothing. A missing resource compiler is a warning, so a
//! cross-compile from macOS or Linux still links, unless `PRINTCRAFT_REQUIRE_WINRES=1` turns it
//! into an error (for release builds).

#[path = "src/windows_manifest.rs"]
mod windows_manifest;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/civilops/civilops-pdf.ico");
    println!("cargo:rerun-if-changed=src/windows_manifest.rs");
    println!("cargo:rerun-if-env-changed=PRINTCRAFT_REQUIRE_WINRES");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/civilops/civilops-pdf.ico")
        .set_manifest(windows_manifest::WINDOWS_MANIFEST)
        .set("ProductName", "CivilOps PDF")
        .set("FileDescription", "CivilOps PDF")
        .set("CompanyName", "CivilOps")
        .set("LegalCopyright", "Copyright (c) 2026 CivilOps")
        .set("OriginalFilename", "civilops-pdf.exe")
        .set("InternalName", "civilops-pdf");
    match res.compile() {
        Ok(()) => leave_out_mingw_default_manifest(),
        Err(e) => {
            if std::env::var_os("PRINTCRAFT_REQUIRE_WINRES").is_some() {
                println!("cargo::error=embedding Windows resources failed: {e}");
                return;
            }
            println!("cargo:warning=civilops-pdf.exe built without icon/version resources: {e}");
        }
    }
}

/// CivilOps PDF: MinGW's GCC links its own `default-manifest.o` into every program. The linker
/// drops it beside ours only when its language is neutral; newer MinGW builds (WinLibs GCC 16) say
/// English (1033), so it warned "multiple non-default manifests" and kept both, or only MinGW's
/// (0.2.2 had no Per-Monitor V2). An empty object of that name in a folder GCC searches first
/// (`-B`) leaves ours the only one, in the program and in the tests that link the same resources.
/// Only for GNU toolchains, whose linker driver is GCC.
fn leave_out_mingw_default_manifest() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("gnu") {
        return;
    }
    let machine: u16 = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => 0x8664,
        Ok("aarch64") => 0xAA64,
        Ok("x86") => 0x014C,
        _ => return,
    };
    let Some(out) = std::env::var_os("OUT_DIR") else { return };
    let dir = std::path::Path::new(&out).join("no-default-manifest");
    // A COFF object header for the target machine, with no sections and no symbols.
    let mut object = machine.to_le_bytes().to_vec();
    object.resize(20, 0);
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(dir.join("default-manifest.o"), &object)) {
        println!("cargo:warning=MinGW's default manifest stays in civilops-pdf.exe beside ours: {e}");
        return;
    }
    println!("cargo:rustc-link-arg=-B{}/", dir.display());
}
