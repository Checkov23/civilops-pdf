//! „Öffnen mit“ für PDF-Dateien im Explorer (Oliver, 18.11.2026).
//!
//! Die installierte Fassung trägt sich beim Installieren und bei jedem Update für den angemeldeten
//! Benutzer ein (`HKEY_CURRENT_USER`, ohne Adminrechte) und beim Deinstallieren wieder aus:
//! - die ProgID [`PROGID`] mit Befehl und Symbol,
//! - `.pdf\OpenWithProgids`: Rechtsklick auf eine PDF ▸ Öffnen mit,
//! - `Applications\civilops-pdf.exe`: der Name in „Andere App auswählen“,
//! - `RegisteredApplications` mit `Capabilities`: Einstellungen ▸ Apps ▸ Standard-Apps.
//!
//! Die Standard-App für PDF ändert sie nicht, die wählt der Nutzer selbst. Die tragbare Fassung
//! lässt die Registry unberührt: Ihr Ordner kann wandern, etwa auf einen USB-Stick, und ein
//! Eintrag zeigte dann ins Leere.

use std::path::Path;

/// Die ProgID der PDF-Dateien von CivilOps PDF.
pub(crate) const PROGID: &str = "CivilOpsPDF.Dokument";

/// So heißt das Programm in „Öffnen mit“ und unter Standard-Apps.
const NAME: &str = "CivilOps PDF";

/// Ein Wert unter `HKEY_CURRENT_USER`: Schlüssel, Name des Werts (leer: Standardwert), Text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Eintrag {
    pub(crate) schluessel: String,
    pub(crate) name: String,
    pub(crate) wert: String,
}

/// Was [`austragen`] löscht: ganze Schlüssel und einzelne Werte in geteilten Schlüsseln
/// (`.pdf\OpenWithProgids` und `RegisteredApplications` gehören auch anderen Programmen).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Austrag {
    pub(crate) schluessel: Vec<String>,
    pub(crate) werte: Vec<(String, String)>,
}

/// Der Dateiname des Programms, so wie Windows es unter `Applications` sucht.
fn dateiname(programm: &Path) -> String {
    programm.file_name().map_or_else(|| "civilops-pdf.exe".to_string(), |n| n.to_string_lossy().into_owned())
}

/// Alle Einträge für das Programm `programm`, unter `basis` (im Betrieb `Software`; Tests nehmen
/// einen eigenen Schlüssel, damit sie den Explorer nicht berühren).
pub(crate) fn eintraege(basis: &str, programm: &Path) -> Vec<Eintrag> {
    let exe = programm.to_string_lossy();
    let befehl = format!("\"{exe}\" \"%1\"");
    let symbol = format!("\"{exe}\",0");
    let klassen = format!("{basis}\\Classes");
    let progid = format!("{klassen}\\{PROGID}");
    let anwendung = format!("{klassen}\\Applications\\{}", dateiname(programm));
    let faehigkeiten = format!("{basis}\\CivilOps\\{NAME}\\Capabilities");
    let e = |schluessel: &str, name: &str, wert: &str| Eintrag { schluessel: schluessel.to_string(), name: name.to_string(), wert: wert.to_string() };
    vec![
        e(&progid, "", "PDF-Dokument"),
        e(&progid, "FriendlyTypeName", "PDF-Dokument"),
        e(&format!("{progid}\\DefaultIcon"), "", &symbol),
        e(&format!("{progid}\\Application"), "ApplicationName", NAME),
        e(&format!("{progid}\\Application"), "ApplicationCompany", "CivilOps"),
        e(&format!("{progid}\\Application"), "ApplicationIcon", &symbol),
        e(&format!("{progid}\\shell"), "", "open"),
        e(&format!("{progid}\\shell\\open\\command"), "", &befehl),
        e(&format!("{klassen}\\.pdf\\OpenWithProgids"), PROGID, ""),
        e(&anwendung, "FriendlyAppName", NAME),
        e(&format!("{anwendung}\\DefaultIcon"), "", &symbol),
        e(&format!("{anwendung}\\shell\\open\\command"), "", &befehl),
        e(&format!("{anwendung}\\SupportedTypes"), ".pdf", ""),
        e(&faehigkeiten, "ApplicationName", NAME),
        e(&faehigkeiten, "ApplicationDescription", "Das PDF-Programm von CivilOps für Pläne und Dokumente"),
        e(&format!("{faehigkeiten}\\FileAssociations"), ".pdf", PROGID),
        e(&format!("{basis}\\RegisteredApplications"), NAME, &faehigkeiten),
    ]
}

/// Was beim Deinstallieren wieder verschwindet: nur unsere Schlüssel und Werte.
pub(crate) fn austrag(basis: &str, programm: &Path) -> Austrag {
    let klassen = format!("{basis}\\Classes");
    Austrag {
        schluessel: vec![
            format!("{klassen}\\{PROGID}"),
            format!("{klassen}\\Applications\\{}", dateiname(programm)),
            format!("{basis}\\CivilOps\\{NAME}"),
        ],
        werte: vec![
            (format!("{klassen}\\.pdf\\OpenWithProgids"), PROGID.to_string()),
            (format!("{basis}\\RegisteredApplications"), NAME.to_string()),
        ],
    }
}

/// Schreibt die Einträge unter `HKEY_CURRENT_USER`.
#[cfg(windows)]
pub(crate) fn eintragen(basis: &str, programm: &Path) -> Result<(), String> {
    use windows_registry::CURRENT_USER;
    for e in eintraege(basis, programm) {
        let schluessel = CURRENT_USER.create(&e.schluessel).map_err(|f| format!("{}: {f}", e.schluessel))?;
        schluessel.set_string(&e.name, &e.wert).map_err(|f| format!("{} {}: {f}", e.schluessel, e.name))?;
    }
    Ok(())
}

/// Löscht unsere Einträge unter `HKEY_CURRENT_USER`. Was schon fehlt, ist kein Fehler.
#[cfg(windows)]
pub(crate) fn austragen(basis: &str, programm: &Path) -> Result<(), String> {
    use windows_registry::CURRENT_USER;
    let a = austrag(basis, programm);
    for (schluessel, name) in &a.werte {
        if let Ok(k) = CURRENT_USER.options().read().write().open(schluessel)
            && k.get_type(name).is_ok()
        {
            k.remove_value(name).map_err(|f| format!("{schluessel} {name}: {f}"))?;
        }
    }
    for schluessel in &a.schluessel {
        if CURRENT_USER.open(schluessel).is_ok() {
            CURRENT_USER.remove_tree(schluessel).map_err(|f| format!("{schluessel}: {f}"))?;
        }
    }
    Ok(())
}

/// Das Programm einer Installation über den Installer; `None` bei der tragbaren Fassung und bei
/// einem Entwicklungsstand.
#[cfg(windows)]
pub(crate) fn installiertes_programm() -> Option<std::path::PathBuf> {
    let ort = velopack::locator::auto_locate_app_manifest(velopack::locator::LocationContext::FromCurrentExe).ok()?;
    (!ort.get_is_portable()).then(|| ort.get_main_exe_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROGRAMM: &str = r"C:\Users\Max Muster\AppData\Local\CivilOpsPDF\current\civilops-pdf.exe";

    fn wert<'a>(alle: &'a [Eintrag], schluessel: &str, name: &str) -> Option<&'a str> {
        alle.iter().find(|e| e.schluessel == schluessel && e.name == name).map(|e| e.wert.as_str())
    }

    #[test]
    fn oeffnen_mit_zeigt_auf_das_programm_mit_leerzeichen_im_pfad() {
        let alle = eintraege("Software", Path::new(PROGRAMM));
        let befehl = format!("\"{PROGRAMM}\" \"%1\"");
        assert_eq!(wert(&alle, r"Software\Classes\CivilOpsPDF.Dokument\shell\open\command", ""), Some(befehl.as_str()));
        assert_eq!(wert(&alle, r"Software\Classes\Applications\civilops-pdf.exe\shell\open\command", ""), Some(befehl.as_str()));
        assert_eq!(wert(&alle, r"Software\Classes\.pdf\OpenWithProgids", PROGID), Some(""));
        assert_eq!(wert(&alle, r"Software\Classes\Applications\civilops-pdf.exe", "FriendlyAppName"), Some("CivilOps PDF"));
        assert_eq!(wert(&alle, r"Software\Classes\Applications\civilops-pdf.exe\SupportedTypes", ".pdf"), Some(""));
        let symbol = format!("\"{PROGRAMM}\",0");
        assert_eq!(wert(&alle, r"Software\Classes\CivilOpsPDF.Dokument\DefaultIcon", ""), Some(symbol.as_str()));
    }

    #[test]
    fn standard_apps_finden_die_faehigkeiten() {
        let alle = eintraege("Software", Path::new(PROGRAMM));
        let pfad = wert(&alle, r"Software\RegisteredApplications", "CivilOps PDF").unwrap();
        assert_eq!(pfad, r"Software\CivilOps\CivilOps PDF\Capabilities");
        assert_eq!(wert(&alle, &format!("{pfad}\\FileAssociations"), ".pdf"), Some(PROGID));
        assert_eq!(wert(&alle, pfad, "ApplicationName"), Some("CivilOps PDF"));
    }

    #[test]
    fn nichts_ausserhalb_der_basis_und_die_standard_app_bleibt() {
        let alle = eintraege("Software", Path::new(PROGRAMM));
        assert!(alle.iter().all(|e| e.schluessel.starts_with("Software\\")));
        // Die Standard-App für PDF wählt der Nutzer: kein Standardwert von `.pdf`, keine UserChoice.
        assert!(!alle.iter().any(|e| e.schluessel == r"Software\Classes\.pdf" || e.schluessel.contains("UserChoice")));
    }

    #[test]
    fn austragen_loescht_nur_unsere_teile_geteilter_schluessel() {
        let a = austrag("Software", Path::new(PROGRAMM));
        assert!(a.schluessel.iter().all(|s| !s.ends_with(r"\.pdf") && !s.ends_with("OpenWithProgids") && !s.ends_with("RegisteredApplications")));
        assert!(a.werte.contains(&(r"Software\Classes\.pdf\OpenWithProgids".to_string(), PROGID.to_string())));
        assert!(a.werte.contains(&(r"Software\RegisteredApplications".to_string(), "CivilOps PDF".to_string())));
        // Jeder geschriebene Schlüssel liegt in einem gelöschten oder ist ein geteilter.
        for e in eintraege("Software", Path::new(PROGRAMM)) {
            let geloescht = a.schluessel.iter().any(|s| e.schluessel == *s || e.schluessel.starts_with(&format!("{s}\\")));
            let geteilt = a.werte.iter().any(|(s, n)| e.schluessel == *s && e.name == *n);
            assert!(geloescht || geteilt, "{} {} bleibt nach dem Deinstallieren stehen", e.schluessel, e.name);
        }
    }

    /// Schreibt und löscht wirklich, aber unter einem eigenen Testschlüssel statt `Software`, damit
    /// der Explorer nichts davon sieht; der Testschlüssel verschwindet am Ende.
    #[cfg(windows)]
    #[test]
    fn eintragen_und_austragen_in_der_registry() {
        use windows_registry::CURRENT_USER;
        let basis = format!(r"Software\CivilOps-PDF-Test-{}", std::process::id());
        let programm = Path::new(PROGRAMM);
        eintragen(&basis, programm).unwrap();
        // Zweimal eintragen (jedes Update) schadet nicht.
        eintragen(&basis, programm).unwrap();
        let befehl = CURRENT_USER.open(format!(r"{basis}\Classes\{PROGID}\shell\open\command")).unwrap().get_string("").unwrap();
        assert_eq!(befehl, format!("\"{PROGRAMM}\" \"%1\""));
        let mit = CURRENT_USER.open(format!(r"{basis}\Classes\.pdf\OpenWithProgids")).unwrap();
        assert_eq!(mit.get_string(PROGID).unwrap(), "");
        // Ein anderes Programm in denselben geteilten Schlüsseln bleibt beim Austragen stehen.
        CURRENT_USER.create(format!(r"{basis}\Classes\.pdf\OpenWithProgids")).unwrap().set_string("Anderes.Programm", "").unwrap();
        austragen(&basis, programm).unwrap();
        austragen(&basis, programm).unwrap();
        assert!(CURRENT_USER.open(format!(r"{basis}\Classes\{PROGID}")).is_err());
        assert!(CURRENT_USER.open(format!(r"{basis}\Classes\Applications\civilops-pdf.exe")).is_err());
        assert!(CURRENT_USER.open(format!(r"{basis}\CivilOps\CivilOps PDF")).is_err());
        let mit = CURRENT_USER.open(format!(r"{basis}\Classes\.pdf\OpenWithProgids")).unwrap();
        assert!(mit.get_type(PROGID).is_err(), "our value is gone");
        assert_eq!(mit.get_string("Anderes.Programm").unwrap(), "", "another program's value stays");
        assert!(CURRENT_USER.open(format!(r"{basis}\RegisteredApplications")).unwrap().get_type("CivilOps PDF").is_err());
        CURRENT_USER.remove_tree(&basis).unwrap();
    }
}
