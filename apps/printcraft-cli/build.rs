//! Windows only: embed version info (VERSIONINFO) into `printcraft-cli.exe`, so Explorer's
//! Details tab, `(Get-Item printcraft-cli.exe).VersionInfo` and inventory tools can read the version
//! and publisher, as they can for `civilops-pdf.exe` (`apps/printcraft/build.rs`).
//!
//! On every other target this does nothing. A missing resource compiler is a warning, so a
//! cross-compile from macOS or Linux still links, unless `PRINTCRAFT_REQUIRE_WINRES=1` turns it
//! into an error (for release builds).

/// The in-box DLLs only printing uses (Windows.Data.Pdf draws the job, prntvpt reads the
/// driver's print capabilities). Trimmed Windows installations (NTLite) can lack them, and a
/// load-time import of a missing DLL stops the program before `main` (#2630). Delay-loaded, they
/// load on the first print; `printcraft_print::spool` checks they exist before calling in.
/// Same list in `apps/printcraft{,-cli}/build.rs`, `crates/print/build.rs` and `crates/print/src/spool.rs`.
const DELAY_LOADED: [&str; 2] = ["windows.data.pdf.dll", "prntvpt.dll"];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PRINTCRAFT_REQUIRE_WINRES");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        for dll in DELAY_LOADED {
            println!("cargo:rustc-link-arg-bins=/DELAYLOAD:{dll}");
        }
        println!("cargo:rustc-link-arg-bins=delayimp.lib");
    }
    let mut res = winresource::WindowsResource::new();
    res.set("ProductName", "CivilOps PDF")
        .set("FileDescription", "CivilOps PDF command-line tool")
        .set("CompanyName", "CivilOps")
        .set("LegalCopyright", "Copyright (c) 2026 CivilOps")
        .set("OriginalFilename", "printcraft-cli.exe")
        .set("InternalName", "printcraft-cli");
    if let Err(e) = res.compile() {
        if std::env::var_os("PRINTCRAFT_REQUIRE_WINRES").is_some() {
            println!("cargo::error=embedding Windows resources failed: {e}");
            return;
        }
        println!("cargo:warning=printcraft-cli.exe built without version resources: {e}");
    }
}
