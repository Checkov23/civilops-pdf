//! Installation und Updates von CivilOps PDF über Velopack, wie bei CivilOps CAD.
//!
//! Ablauf:
//! - [`beim_start`] läuft als Erstes in `main`. Es bedient die Aufrufe des Installers
//!   (Installation, Update, Deinstallation) und spielt ein schon geladenes Update ein, bevor das
//!   Programm richtig startet, aber nur, wenn keine andere Sitzung läuft (die würde sonst gestört).
//! - [`pruefen_und_laden`] sucht im Hintergrund in der Update-Quelle nach einer neueren Fassung
//!   und lädt sie. Eingespielt wird sie beim nächsten Start oder nach dem Beenden über
//!   [`nach_dem_beenden_einspielen`], nie mitten in einer Sitzung.
//!
//! Die Update-Quelle steht in `aktualisierung.txt` neben dem Programm (schreibt das Release-Skript
//! `packaging/civilops/release.py`) oder in der Umgebungsvariable `CIVILOPS_PDF_UPDATE_QUELLE`:
//! ein Ordner oder eine https-Adresse mit den Dateien von `vpk pack`. Für geschützte Ordner steht
//! dort zusätzlich `zugang = benutzer:passwort` (Basic-Anmeldung, nur über https), oder in
//! `CIVILOPS_PDF_UPDATE_ZUGANG`. Der Kanal (zum Beispiel `test`) steckt im installierten Paket.

use std::fs::{File, TryLockError};
use std::path::PathBuf;
use std::sync::OnceLock;

use velopack::{UpdateCheck, UpdateManager, VelopackApp, sources};

/// Ein geladenes Update, das beim nächsten Start eingespielt wird.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Update {
    /// Versionsnummer, zum Beispiel `0.1.2`.
    pub version: String,
    /// Versionshinweise aus dem Paket (Markdown, kann leer sein).
    pub notizen: String,
}

/// Die Sperrdatei dieser Sitzung, geteilt gesperrt bis zum Prozessende: Solange irgendeine
/// Sitzung läuft, bekommt ein neuer Start die alleinige Sperre nicht und spielt nichts ein.
static SITZUNG: OnceLock<File> = OnceLock::new();

/// Als Erstes in `main` aufrufen. Beendet den Prozess selbst, wenn der Installer das Programm nur
/// für einen Hook aufruft oder ein geladenes Update eingespielt wird (das Programm startet danach
/// neu).
pub fn beim_start() {
    let sperre = sperrdatei();
    let andere_sitzung = sperre.as_ref().is_some_and(andere_sitzung_laeuft);
    let app = VelopackApp::build().set_auto_apply_on_startup(!andere_sitzung);
    // Nach einem Update (der Installer ruft das neue Programm mit `--veloapp-updated`) sollen
    // Desktop, Startmenü und Taskleiste das neue Symbol zeigen.
    #[cfg(windows)]
    let app = app.on_after_update_fast_callback(|neu| symbole_auffrischen(&neu.to_string()));
    let mut app = app;
    app.run();
    if let Some(datei) = sperre {
        // Ab jetzt sehen andere Starts: hier läuft eine Sitzung. Klappt das nicht, fehlt nur der
        // Schutz für die andere Sitzung; das Programm selbst läuft weiter.
        let _ = datei.try_lock_shared();
        let _ = SITZUNG.set(datei);
    }
}

/// Verknüpfungen zeigen auf `current\civilops-pdf.exe` und bekommen mit dem Update dessen neues
/// Symbol, Windows zeigt aber bis zum Erneuern seines Zwischenspeichers das alte.
/// `ie4uinit.exe -show` erneuert ihn (Windows 10 und 11) und meldet es der Oberfläche. Ohne
/// Fenster; gewartet wird höchstens fünf Sekunden, der Hook darf nicht lange laufen (Velopack
/// beendet ihn nach 15 Sekunden). Fehlt das Programm, bleibt es beim alten Bild bis zur nächsten
/// Anmeldung.
#[cfg(windows)]
fn symbole_auffrischen(version: &str) {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let system = std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    let programm = system.join("System32").join("ie4uinit.exe");
    let start = Command::new(&programm)
        .arg("-show")
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let ergebnis = match start {
        Ok(mut kind) => {
            let ende = Instant::now() + Duration::from_secs(5);
            loop {
                match kind.try_wait() {
                    Ok(Some(_)) => break "Symbole aufgefrischt",
                    Ok(None) if Instant::now() < ende => std::thread::sleep(Duration::from_millis(50)),
                    Ok(None) | Err(_) => break "Symbole werden aufgefrischt",
                }
            }
        }
        Err(_) => "ie4uinit.exe nicht gefunden, Symbole erst nach der nächsten Anmeldung neu",
    };
    protokoll(&format!("Update {version} eingespielt: {ergebnis}"));
}

/// Ob eine andere Sitzung die Sperrdatei hält. Ein Fehler beim Sperren zählt als „keine“.
fn andere_sitzung_laeuft(datei: &File) -> bool {
    match datei.try_lock() {
        Ok(()) => {
            let _ = datei.unlock();
            false
        }
        Err(TryLockError::WouldBlock) => true,
        Err(TryLockError::Error(_)) => false,
    }
}

/// `sitzung.lock` im Installationsordner, nur bei einer Installation (oder tragbaren Fassung).
fn sperrdatei() -> Option<File> {
    installierte_version()?;
    let ordner = installationsordner()?;
    std::fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(ordner.join("sitzung.lock")).ok()
}

/// Installierte Version, zum Beispiel `0.1.1`. `None` bei einem Entwicklungsstand, der nicht über
/// den Installer (oder die tragbare Fassung) kam.
pub fn installierte_version() -> Option<String> {
    static VERSION: OnceLock<Option<String>> = OnceLock::new();
    VERSION.get_or_init(|| UpdateManager::new(sources::NoneSource {}, None, None).ok().map(|um| um.get_current_version_as_string())).clone()
}

/// Update-Quelle: Umgebungsvariable `CIVILOPS_PDF_UPDATE_QUELLE`, sonst `aktualisierung.txt`
/// neben dem Programm.
pub fn quelle() -> Option<String> {
    eintrag("CIVILOPS_PDF_UPDATE_QUELLE", "quelle")
}

/// Zugang für eine geschützte Quelle (`benutzer:passwort`).
fn zugang() -> Option<String> {
    eintrag("CIVILOPS_PDF_UPDATE_ZUGANG", "zugang")
}

fn eintrag(variable: &str, schluessel: &str) -> Option<String> {
    if let Ok(v) = std::env::var(variable)
        && !v.trim().is_empty()
    {
        return Some(v.trim().to_string());
    }
    let text = std::fs::read_to_string(programmordner()?.join("aktualisierung.txt")).ok()?;
    wert(&text, schluessel)
}

/// Sucht nach einer neueren Fassung und lädt sie herunter. Blockiert, also im Hintergrund
/// aufrufen. `Ok(None)`: nichts zu tun (aktuell, nicht installiert oder keine Quelle eingetragen).
/// Das Ergebnis steht zusätzlich in `aktualisierung.log` im Installationsordner.
pub fn pruefen_und_laden() -> Result<Option<Update>, String> {
    if installierte_version().is_none() {
        return Ok(None);
    }
    let Some(quelle) = quelle() else {
        protokoll("keine Update-Quelle eingetragen");
        return Ok(None);
    };
    let ergebnis = pruefen_und_laden_aus(&quelle);
    match &ergebnis {
        Ok(Some(u)) => protokoll(&format!("{quelle}: Update {} geladen", u.version)),
        Ok(None) => protokoll(&format!("{quelle}: aktuell")),
        Err(e) => protokoll(&format!("{quelle}: Fehler {e}")),
    }
    ergebnis
}

fn pruefen_und_laden_aus(quelle: &str) -> Result<Option<Update>, String> {
    let um = if quelle.starts_with("https://") || quelle.starts_with("http://") {
        // Große Pakete über langsame Leitungen: 30 Minuten je Anfrage.
        let mut optionen = velopack::HttpOptions { TimeoutMilliseconds: 30 * 60 * 1000, ..Default::default() };
        if let Some(z) = zugang() {
            optionen.Headers.push(velopack::HttpHeader { Name: "Authorization".to_string(), Value: format!("Basic {}", base64(z.as_bytes())) });
        }
        UpdateManager::new(sources::HttpSource::new_with_options(quelle, optionen), None, None)
    } else {
        UpdateManager::new(sources::FileSource::new(quelle), None, None)
    }
    .map_err(|e| e.to_string())?;

    // Schon geladen, aber noch nicht eingespielt (weil eine andere Sitzung lief).
    if let Some(wartend) = um.get_update_pending_restart() {
        return Ok(Some(Update { version: wartend.Version, notizen: wartend.NotesMarkdown }));
    }
    match um.check_for_updates().map_err(|e| e.to_string())? {
        UpdateCheck::UpdateAvailable(info) => {
            um.download_updates(&info, None).map_err(|e| e.to_string())?;
            let ziel = &info.TargetFullRelease;
            Ok(Some(Update { version: ziel.Version.clone(), notizen: ziel.NotesMarkdown.clone() }))
        }
        UpdateCheck::NoUpdateAvailable | UpdateCheck::RemoteIsEmpty => Ok(None),
    }
}

/// Startet den Velopack-Updater, der wartet, bis dieses Programm sich beendet hat (höchstens 60 s),
/// das geladene Update einspielt und das Programm neu startet. Danach muss das Programm sich
/// regulär beenden; vorher muss alles gespeichert sein.
pub fn nach_dem_beenden_einspielen() -> Result<(), String> {
    let um = UpdateManager::new(sources::NoneSource {}, None, None).map_err(|e| e.to_string())?;
    let wartend = um.get_update_pending_restart().ok_or_else(|| "no downloaded update".to_string())?;
    protokoll(&format!("Update {} wird nach dem Beenden eingespielt", wartend.Version));
    um.wait_exit_then_apply_updates(&wartend, false, true, Vec::<String>::new()).map_err(|e| e.to_string())
}

/// Base64 (Standard-Alphabet mit Auffüllung) für die Basic-Anmeldung.
fn base64(daten: &[u8]) -> String {
    const ZEICHEN: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let zeichen = |n: u32| ZEICHEN.get((n & 63) as usize).map_or('=', |&c| char::from(c));
    let mut aus = String::with_capacity(daten.len().div_ceil(3).saturating_mul(4));
    for block in daten.chunks(3) {
        let byte = |i: usize| u32::from(block.get(i).copied().unwrap_or(0));
        let n = (byte(0) << 16) | (byte(1) << 8) | byte(2);
        aus.push(zeichen(n >> 18));
        aus.push(zeichen(n >> 12));
        aus.push(if block.len() > 1 { zeichen(n >> 6) } else { '=' });
        aus.push(if block.len() > 2 { zeichen(n) } else { '=' });
    }
    aus
}

fn programmordner() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.to_path_buf())
}

/// Eine Ebene über dem Programmordner: bei einer Installation `%LOCALAPPDATA%\CivilOpsPDF`, bei
/// der tragbaren Fassung der entpackte Ordner.
fn installationsordner() -> Option<PathBuf> {
    programmordner()?.parent().map(PathBuf::from)
}

/// Eine Zeile in `aktualisierung.log` im Installationsordner, Zeit in Sekunden seit 1970. Fehler
/// beim Schreiben sind egal.
fn protokoll(zeile: &str) {
    use std::io::Write;
    let Some(ordner) = installationsordner() else {
        return;
    };
    let sekunden = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(ordner.join("aktualisierung.log")) {
        let _ = writeln!(f, "{sekunden} {zeile}");
    }
}

/// Wert zu `schluessel = wert` (Anführungszeichen optional, `#` leitet Kommentare ein).
fn wert(text: &str, schluessel: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .filter(|z| !z.starts_with('#'))
        .filter_map(|z| z.split_once('='))
        .filter(|(k, _)| k.trim() == schluessel)
        .map(|(_, v)| v.trim().trim_matches('"').trim().to_string())
        .find(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quelle_aus_textdatei() {
        let text = "# Update-Quelle\nkanal = test\nquelle = \"C:\\\\Releases\\\\test\"\n";
        assert_eq!(wert(text, "quelle").as_deref(), Some("C:\\\\Releases\\\\test"));
        assert_eq!(wert(text, "kanal").as_deref(), Some("test"));
        assert_eq!(wert("quelle = https://download.civilops.de/pdf-test", "quelle").as_deref(), Some("https://download.civilops.de/pdf-test"));
        assert_eq!(wert("zugang = pdf:a=b", "zugang").as_deref(), Some("pdf:a=b"), "only the first = splits");
        assert_eq!(wert("# quelle = x\nquelle =   \n", "quelle"), None);
    }

    #[test]
    fn base64_wie_rfc4648() {
        assert_eq!(base64(b"Aladdin:open sesame"), "QWxhZGRpbjpvcGVuIHNlc2FtZQ==");
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn entwicklungsstand_ohne_installation() {
        // Im Testlauf ist nichts installiert: keine Version, kein Update, keine Sperrdatei.
        assert_eq!(installierte_version(), None);
        assert_eq!(pruefen_und_laden(), Ok(None));
        assert!(sperrdatei().is_none());
        assert!(nach_dem_beenden_einspielen().is_err());
    }

    #[test]
    fn eine_laufende_sitzung_haelt_die_sperre() {
        let pfad = std::env::temp_dir().join(format!("civilops-pdf-sperre-{}.lock", std::process::id()));
        let oeffnen = || std::fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(&pfad).unwrap();
        let laufend = oeffnen();
        let neuer_start = oeffnen();
        assert!(!andere_sitzung_laeuft(&neuer_start), "nobody holds it yet");
        laufend.try_lock_shared().unwrap();
        assert!(andere_sitzung_laeuft(&neuer_start), "a running session holds a shared lock");
        // Zwei Sitzungen nebeneinander: beide halten die geteilte Sperre.
        neuer_start.try_lock_shared().unwrap();
        drop(laufend);
        let dritter = oeffnen();
        assert!(andere_sitzung_laeuft(&dritter), "the second session still runs");
        drop(neuer_start);
        assert!(!andere_sitzung_laeuft(&dritter));
        drop(dritter);
        let _ = std::fs::remove_file(&pfad);
    }
}
