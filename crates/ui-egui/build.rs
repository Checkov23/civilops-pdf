//! Embed the interface catalogs (craft-i18n's `locales/<code>/{common,pdfcraft}.tsv`) into the
//! binary. Nothing is read at run time.
//!
//! CivilOps PDF: the catalogs are in the repository, `vendor/craft-i18n` (storytold/craft-i18n at
//! the SHA in `craft-i18n.rev`, with CivilOps PDF's name and rows, see its README); `CRAFT_I18N_DIR`
//! may name a craft-i18n checkout instead. `craft_i18n.rs` holds `CRAFT_I18N`, one (locale, shared
//! catalog, app catalog) entry per locale that has a `pdfcraft.tsv`. Without any the interface is
//! English only (a warning; with `CRAFT_I18N_REQUIRED=1`, an error). Upstream's contributor credits
//! are not built in (CivilOps PDF's About has none).

use std::fmt::Write as _;

fn main() {
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("craft_i18n.rs");
    if let Err(e) = std::fs::write(&out, craft_i18n()) {
        println!("cargo::error=writing {}: {e}", out.display());
    }
}

/// The catalog this app reads in each locale: `locales/<locale>/pdfcraft.tsv`, by the name it has in
/// craft-i18n (CivilOps PDF keeps upstream's file name, so the catalogs update as plain copies).
const APP_CATALOG: &str = "pdfcraft.tsv";

/// `CRAFT_I18N` from `CRAFT_I18N_DIR`: the `Embedded` list the craft-i18n crate reads.
fn craft_i18n() -> String {
    println!("cargo::rerun-if-env-changed=CRAFT_I18N_DIR");
    println!("cargo::rerun-if-env-changed=CRAFT_I18N_REQUIRED");
    let required = std::env::var("CRAFT_I18N_REQUIRED").is_ok_and(|v| v.trim() == "1");
    // CivilOps PDF: the catalogs are in the repository (vendor/craft-i18n); CRAFT_I18N_DIR may
    // name a craft-i18n checkout instead.
    let dir = std::env::var_os("CRAFT_I18N_DIR")
        .filter(|d| !d.is_empty())
        .or_else(|| Some(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/craft-i18n").into_os_string()));
    let entries = match dir {
        // Not canonicalized: on Windows that gives `\\?\C:\…`, which `include_str!` can't read.
        Some(dir) => match locales(&std::path::PathBuf::from(dir).join("locales")) {
            Ok(entries) => entries,
            Err(e) => {
                let level = if required { "error" } else { "warning" };
                println!("cargo::{level}=CRAFT_I18N_DIR: {e}; the interface is English only");
                Vec::new()
            }
        },
        None => {
            if required {
                println!("cargo::error=CRAFT_I18N_REQUIRED=1 but CRAFT_I18N_DIR is unset: a release must embed the translations");
            }
            Vec::new()
        }
    };
    let mut o = String::from("/// (locale, shared catalog, CivilOps PDF's catalog), one entry per locale; `\"\"` for a missing file.\n");
    o.push_str("pub static CRAFT_I18N: &craft_i18n::Embedded = &[\n");
    for (code, shared, app) in entries {
        // `{:?}` escapes the path (Windows backslashes) as a Rust string literal.
        let shared = shared.map_or_else(|| "\"\"".to_string(), |p| format!("include_str!({p:?})"));
        let _ = writeln!(o, "    ({code:?}, {shared}, include_str!({app:?})),");
    }
    o.push_str("];\n");
    o
}

/// One embedded locale: its code, its `common.tsv` if it has one, and its PrintCraft catalog.
type Locale = (String, Option<std::path::PathBuf>, std::path::PathBuf);

/// Every locale under `dir` with PrintCraft's catalog, by code. An error when `dir` isn't a
/// craft-i18n `locales/` directory.
fn locales(dir: &std::path::Path) -> Result<Vec<Locale>, String> {
    // Build scripts run in the crate's directory and `include_str!` resolves against the
    // generated file, so a relative path would mean two different places.
    if !dir.is_absolute() {
        return Err(format!("{} is not an absolute path", dir.display()));
    }
    // Before read_dir, so a checkout that appears later reruns the script.
    println!("cargo::rerun-if-changed={}", dir.display());
    let list = std::fs::read_dir(dir).map_err(|e| format!("{} is not a craft-i18n checkout ({e})", dir.display()))?;
    let mut out = Vec::new();
    for entry in list.flatten() {
        let path = entry.path();
        let Some(code) = path.file_name().and_then(|n| n.to_str()).map(str::to_string) else { continue };
        let app = path.join(APP_CATALOG);
        if !app.is_file() {
            continue;
        }
        println!("cargo::rerun-if-changed={}", app.display());
        let shared = Some(path.join("common.tsv")).filter(|p| p.is_file());
        if let Some(p) = &shared {
            println!("cargo::rerun-if-changed={}", p.display());
        }
        out.push((code, shared, app));
    }
    if out.is_empty() {
        return Err(format!("{} has no locale with a {APP_CATALOG}", dir.display()));
    }
    out.sort();
    Ok(out)
}
