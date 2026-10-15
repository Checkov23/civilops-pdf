//! Portable mode (#157): a marker file beside the executable ([`MARKERS`]) keeps the settings,
//! log files, crash-recovery autosaves and new digital IDs in `<exe dir>/CivilOpsPDFData`, so a
//! portable copy (on a USB stick, say) writes nothing to `%APPDATA%` or `%LOCALAPPDATA%`. The
//! Windows portable zip ships with `portable.txt`; the MSI doesn't. PhotoCraft uses the same
//! scheme (`photocraft/apps/photocraft/src/app_dirs.rs`).
//!
//! If that folder can't be written (read-only medium, Program Files), portable mode stays off, the
//! per-user folders are used and the app says why.
//!
//! CivilOps PDF ships its portable zip through Velopack instead: the program runs from
//! `<folder>/current/`, which every update replaces, and Velopack's own marker `.portable` sits in
//! `<folder>`. That layout is portable mode too, with the data folder in `<folder>`, so the
//! settings survive updates and travel with the folder.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Files beside the executable that switch on portable mode (either one; contents are ignored).
pub const MARKERS: [&str; 2] = ["portable.txt", "CivilOpsPDF.portable"];
/// The data folder created beside the executable in portable mode.
pub const DATA_DIR: &str = "CivilOpsPDFData";
/// CivilOps PDF: Velopack's marker in the root of its portable zip.
pub const ZIP_MARKER: &str = ".portable";
/// CivilOps PDF: the folder of the portable zip the program runs from (replaced by each update).
pub const ZIP_PROGRAM_DIR: &str = "current";

/// The outcome of looking for a portable marker.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Portable {
    /// `<exe dir>/CivilOpsPDFData` when portable mode is on, `None` otherwise.
    pub dir: Option<PathBuf>,
    /// Set when a marker was found but its data folder can't be written.
    pub unwritable: Option<Unwritable>,
    /// CivilOps PDF: portable mode comes from the layout of the portable zip (data folder beside
    /// `current/`). Its first start there takes over the settings of the user folder.
    pub zip: bool,
}

/// A marker was found, but the data folder beside it can't be created or written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unwritable {
    pub marker: PathBuf,
    pub folder: PathBuf,
    /// The operating system's error text.
    pub error: String,
}

impl std::fmt::Display for Unwritable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "portable mode is off: {} was found, but {} can't be written ({}); settings are kept in the user folder instead",
            self.marker.display(),
            self.folder.display(),
            self.error
        )
    }
}

/// Portable mode for this process, resolved once (the check probes the disk).
pub fn current() -> &'static Portable {
    static CURRENT: OnceLock<Portable> = OnceLock::new();
    CURRENT.get_or_init(|| {
        let exe = std::env::current_exe().ok();
        let portable = resolve(exe.as_deref().and_then(Path::parent));
        if let Some(dir) = &portable.dir {
            log::info!("portable mode: settings are kept in {}", dir.display());
        }
        if let Some(unwritable) = &portable.unwritable {
            log::warn!("{unwritable}");
        }
        portable
    })
}

/// The portable data folder, when portable mode is on.
pub fn data_dir() -> Option<&'static Path> {
    current().dir.as_deref()
}

/// Look for a marker in `exe_dir` and, when one is there, create its data folder beside it. Without
/// one, the layout of CivilOps PDF's portable zip counts too (data folder beside `current/`).
pub fn resolve(exe_dir: Option<&Path>) -> Portable {
    let Some(exe_dir) = exe_dir else { return Portable::default() };
    let (marker, base, zip) = match (find_marker(exe_dir), zip_root(exe_dir)) {
        (Some(marker), _) => (marker, exe_dir.to_path_buf(), false),
        (None, Some(root)) => (root.join(ZIP_MARKER), root, true),
        (None, None) => return Portable::default(),
    };
    let folder = base.join(DATA_DIR);
    match ensure_writable(&folder) {
        Ok(()) => Portable { dir: Some(folder), unwritable: None, zip },
        Err(e) => Portable { dir: None, unwritable: Some(Unwritable { marker, folder, error: e.to_string() }), zip: false },
    }
}

/// The first marker file in `exe_dir`, if any. A folder with a marker's name is not a marker.
pub fn find_marker(exe_dir: &Path) -> Option<PathBuf> {
    MARKERS.iter().map(|m| exe_dir.join(m)).find(|p| p.is_file())
}

/// CivilOps PDF: the root of the portable zip when `exe_dir` is its `current/` folder (the root
/// holds Velopack's `.portable` file).
fn zip_root(exe_dir: &Path) -> Option<PathBuf> {
    if exe_dir.file_name()? != ZIP_PROGRAM_DIR {
        return None;
    }
    let root = exe_dir.parent()?;
    root.join(ZIP_MARKER).is_file().then(|| root.to_path_buf())
}

/// Create `dir` and prove a file can be written in it (a folder can exist yet be read-only).
fn ensure_writable(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let probe = dir.join(".write-test");
    std::fs::write(&probe, b"")?;
    // A leftover probe is harmless; don't fail portable mode over it.
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp(tag: &str) -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!("printcraft-portable-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn no_marker_leaves_portable_mode_off() {
        let exe = temp("off");
        assert_eq!(resolve(Some(&exe)), Portable::default());
        assert!(!exe.join(DATA_DIR).exists(), "no data folder without a marker");
        let _ = std::fs::remove_dir_all(&exe);
    }

    #[test]
    fn either_marker_keeps_the_data_beside_the_exe() {
        for marker in MARKERS {
            let exe = temp("on");
            std::fs::write(exe.join(marker), b"").unwrap();
            let p = resolve(Some(&exe));
            assert_eq!(p.unwritable, None, "{marker}");
            let dir = p.dir.unwrap();
            assert_eq!(dir, exe.join(DATA_DIR));
            assert!(dir.is_dir(), "the data folder is created");
            assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "the write probe is removed");
            let _ = std::fs::remove_dir_all(&exe);
        }
    }

    #[test]
    fn an_existing_data_folder_is_reused() {
        let exe = temp("reuse");
        std::fs::write(exe.join("portable.txt"), b"PrintCraft portable mode").unwrap();
        std::fs::create_dir_all(exe.join(DATA_DIR)).unwrap();
        std::fs::write(exe.join(DATA_DIR).join("app.ron"), b"()").unwrap();
        assert_eq!(resolve(Some(&exe)).dir, Some(exe.join(DATA_DIR)));
        assert_eq!(std::fs::read(exe.join(DATA_DIR).join("app.ron")).unwrap(), b"()", "existing settings are kept");
        let _ = std::fs::remove_dir_all(&exe);
    }

    #[test]
    fn a_marker_folder_is_not_a_marker() {
        let exe = temp("dir-marker");
        std::fs::create_dir_all(exe.join("portable.txt")).unwrap();
        assert_eq!(resolve(Some(&exe)), Portable::default());
        let _ = std::fs::remove_dir_all(&exe);
    }

    #[test]
    fn unwritable_data_folder_turns_portable_mode_off_and_says_why() {
        let exe = temp("unwritable");
        std::fs::write(exe.join("portable.txt"), b"").unwrap();
        // A file where the data folder should go: it can't be created, on every platform.
        std::fs::write(exe.join(DATA_DIR), b"in the way").unwrap();
        let p = resolve(Some(&exe));
        assert_eq!(p.dir, None);
        let unwritable = p.unwritable.unwrap();
        assert_eq!(unwritable.marker, exe.join("portable.txt"));
        assert_eq!(unwritable.folder, exe.join(DATA_DIR));
        let message = unwritable.to_string();
        assert!(message.contains("portable mode is off") && message.contains("can't be written"), "{message}");
        let _ = std::fs::remove_dir_all(&exe);
    }

    #[test]
    fn no_exe_dir_leaves_portable_mode_off() {
        assert_eq!(resolve(None), Portable::default());
    }

    /// CivilOps PDF's portable zip: `<root>/.portable` and the program in `<root>/current/`.
    fn zip(tag: &str) -> (PathBuf, PathBuf) {
        let root = temp(tag);
        let current = root.join(ZIP_PROGRAM_DIR);
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(root.join(ZIP_MARKER), b"").unwrap();
        (root, current)
    }

    #[test]
    fn the_portable_zip_keeps_its_data_beside_the_program_folder() {
        let (root, current) = zip("zip");
        let p = resolve(Some(&current));
        assert_eq!(p, Portable { dir: Some(root.join(DATA_DIR)), unwritable: None, zip: true });
        assert!(root.join(DATA_DIR).is_dir(), "outside current/, which updates replace");
        assert!(!current.join(DATA_DIR).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_marker_beside_the_program_still_wins_in_the_zip() {
        let (root, current) = zip("zip-marker");
        std::fs::write(current.join("portable.txt"), b"").unwrap();
        let p = resolve(Some(&current));
        assert_eq!((p.dir, p.zip), (Some(current.join(DATA_DIR)), false));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_the_zip_layout_counts() {
        // A `.portable` beside a folder of another name, or a `current/` without it: not the zip.
        let (root, current) = zip("zip-other");
        let other = root.join("bin");
        std::fs::create_dir_all(&other).unwrap();
        assert_eq!(resolve(Some(&other)), Portable::default());
        std::fs::remove_file(root.join(ZIP_MARKER)).unwrap();
        assert_eq!(resolve(Some(&current)), Portable::default());
        assert!(!root.join(DATA_DIR).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unwritable_zip_folder_says_why() {
        let (root, current) = zip("zip-unwritable");
        std::fs::write(root.join(DATA_DIR), b"in the way").unwrap();
        let p = resolve(Some(&current));
        assert_eq!((p.dir.clone(), p.zip), (None, false));
        assert_eq!(p.unwritable.map(|u| u.marker), Some(root.join(ZIP_MARKER)));
        let _ = std::fs::remove_dir_all(&root);
    }
}
