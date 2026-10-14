# CivilOps PDF

Das PDF-Programm von CivilOps für Pläne und Dokumente: ansehen, messen, ändern, kommentieren,
zusammenstellen, unterschreiben und drucken. Läuft lokal unter Windows, ohne Konto und ohne Cloud.

## Was CivilOps PDF kann

- **Werkzeugleiste oben** (wie Bauabrechnung und CAD von CivilOps): Auswählen, Hand, Text, Objekte,
  Länge, Fläche, Maßstab, Notiz, Hervorheben, Wolke, Mehr, Drucken, Speichern. Menüs in der
  Reiterleiste, der Plan füllt das Fenster, alles Übrige unter „Alle Werkzeuge“.
- **Messen mit Maßstab**: Länge (zwei oder mehr Punkte), Fläche, Maßstab 1:n oder kalibriert an
  einer bekannten Strecke. Der Punkt rastet auf Linienenden des Plans ein (Alt schaltet das ab).
  Der Maßstab wird als PDF-Viewport gespeichert, die Messungen als normgerechte Mess-Kommentare
  (`/Measure`), so dass andere PDF-Programme sie ebenfalls verstehen. Zahlen deutsch: `12,34 m`,
  `1.234,56 m²`.
- **Objekte ändern** (Werkzeug „Objekte“): alles, was eine Seite zeichnet, also Linien, Flächen,
  Schraffuren, Bilder, Farbverläufe, Texte und Blöcke. Wählen per Klick, Doppelklick (ganzer
  Linienzug oder ganze Schraffur), Rahmen wie im CAD und Strg+A; verschieben, Größe ändern,
  drehen, spiegeln, löschen, Linien- und Füllfarbe, Linienstärke in mm, Linienart, Deckkraft,
  Blöcke auflösen.
- **Texte ändern** (Werkzeug „Text“): Beschriftungen direkt im Plan überschreiben, auch auf
  AutoCAD-Plänen; „Text formatieren“ für Schrift, Größe, Farbe, Ausrichtung und Abstände.
- **Texterkennung** über die Texterkennung von Windows (Deutsch mit Umlauten und alle anderen
  dort installierten Sprachen), Bearbeiten ▸ Text erkennen.
- **Kommentare, Formulare, Seiten ordnen, Schützen, Unterschreiben, Schwärzen, Exportieren** und
  mehr unter „Alle Werkzeuge“.
- **Deutsche Oberfläche** mit „Sie“ und den Begriffen von CivilOps; dazu 21 weitere Sprachen
  (Einstellungen ▸ Sprache der Oberfläche).
- **Drucken** mit Vorschau je Blatt: Originalgröße (maßstabsgetreu), Anpassen, Poster,
  mehrere Seiten je Blatt, Broschüre, Exemplare, beidseitig, Graustufen und die
  Einstellungen des Druckertreibers.
- **Ein Fenster**: Dateien, die Explorer oder Outlook öffnen, kommen als neuer Reiter in das
  laufende Programm. Auf der Startseite lassen sich Ordner anheften (zum Beispiel der des
  Scanners), die ihre neuesten PDFs zeigen.
- **Spenden**: Hilfe ▸ Spenden, im Fenster „Über“ und auf der Startseite (PayPal).
- **Installer und Updates** (Velopack): Installation pro Benutzer ohne Adminrechte. Beim ersten
  Start fragt das Programm, ob es beim Start nach Updates suchen darf (abschaltbar unter
  Einstellungen); eine neuere Fassung lädt es im Hintergrund und meldet sie, eingespielt wird
  beim nächsten Start oder mit „Jetzt neu starten“ (nur, wenn alles gespeichert ist).

## Bauen

```
cargo build --release -p printcraft
```

Ergebnis: `target/release/civilops-pdf.exe`. Prüfen vor jedem Commit: `cargo fmt --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, dazu
`cargo xtask layers`, `cargo xtask wasm`, `cargo xtask assets` und `cargo xtask parity`.

## Auslieferung und Updates

Version an zwei Stellen hochzählen (`apps/printcraft/Cargo.toml`, `crates/engine/src/links.rs`),
dann:

```
python packaging/civilops/release.py --version 0.1.5 --notizen-datei notizen.md --hochladen
```

Das Skript baut, legt `Drittanbieter-Lizenzen.txt` in den Programmordner
(`packaging/civilops/drittanbieter.py`), packt mit `vpk` 1.2.158 (aus `../civilops-cad-werkzeuge/vpk`)
und lädt in den geschützten Kanal `https://download.civilops.de/pdf-test` (Zugang in
`../civilops-pdf-releases/zugang-test.txt`, nie ins Repo; eigene Passwortdatei auf dem Server,
`.htaccess` aus `packaging/civilops/download-test.htaccess`; SSH-Ziel und Schlüssel in
`../civilops-pdf-releases/server.txt`, ebenfalls nie ins Repo). Installierte Programme holen sich
die neue Fassung beim Start. Öffentlicher Kanal für die Freeware: `--kanal stabil --hochladen
--oeffentlich` lädt nach `https://download.civilops.de/pdf` (ohne Anmeldung, ohne Zugang im Paket).
Installer und tragbare Fassung mit Versionsnummer liegen danach in
`../civilops-pdf-releases/web/test`. Fassungen des Testkanals enthalten dessen Zugang und kommen
deshalb nicht in öffentliche GitHub-Releases; dorthin gehören nur Fassungen des öffentlichen
Kanals. Das Protokoll der Update-Prüfung steht in `%LOCALAPPDATA%\CivilOpsPDF\aktualisierung.log`.

## Aufbau

| Bereich | Ort |
|---|---|
| Programm (`civilops-pdf.exe`) | `apps/printcraft` |
| PDF lesen und schreiben | `crates/cos`, `crates/model`, `crates/content`, `crates/filters`, `crates/crypt` |
| Darstellung | `crates/render` (hayro, in `vendor/`) |
| Oberfläche | `crates/ui-egui` (egui, in `vendor/egui` mit Übersetzungshaken) |
| Werkzeugleiste | `crates/ui-egui/src/tool_bar.rs`, Menüs in `chrome.rs`, Farben in `theme.rs` |
| Messen | `crates/annot/src/measure.rs`, `crates/ui-egui/src/plan_measure_ui.rs` (die Messwerkzeuge von PDFCraft, `crates/measure` und `measure_ui.rs`, bleiben Befehle ohne eigenen Platz) |
| Objekte | `crates/edit/src/plan_objects.rs`, `crates/ui-egui/src/objects_ui.rs` |
| Texte | `crates/edit/src/text.rs`, `crates/ui-egui/src/edit_text_ui.rs` |
| Texterkennung | `crates/ocr` (Windows: `win.rs`) |
| Deutsch | `crates/ui-egui/src/i18n_de.rs` (gilt vor der deutschen Übersetzung von PDFCraft, `crates/ui-egui/src/i18n/de.tsv`); weitere Sprachen in `crates/ui-egui/src/i18n/` |
| Drucken | `crates/print`, `crates/ui-egui/src/print_ui.rs` (unter Windows über craft-print) |
| Automatik und Kommandozeile | `crates/automation`, `apps/printcraft-cli` |
| Installer und Updates | `crates/aktualisierung` (Velopack), `crates/ui-egui/src/updates.rs`, `packaging/civilops/` |
| Erscheinungsbild | `assets/civilops/` |

## Abgleich mit PDFCraft

CivilOps PDF ist aus PrintCraft entstanden, das heute PDFCraft heißt. Neuerungen von dort kommen
gesammelt herein: Die Umbenennung wird zurückgedreht (PdfCraft → PrintCraft), dann wird mit
dem zuletzt übernommenen Stand als Basis zusammengeführt und das Ergebnis als ein Commit
veröffentlicht. Werkzeugleiste, Messen, Objekte, Deutsch, Texterkennung, Updates und Symbole
bleiben die von CivilOps PDF. Zuletzt übernommen: PDFCraft 0.7.1 (Stand `7437bde`).

## Lizenz

Der Code steht unter der MIT-Lizenz (`LICENSE-MIT`, mit allen nötigen Urheberrechtsvermerken).
Fremde Bestandteile wie Schriften, Symbole und Bibliotheken stehen mit ihren Lizenzen in `NOTICE`
und `ATTRIBUTION.md`; jedes Release legt alle Lizenztexte als `Drittanbieter-Lizenzen.txt` in den
Programmordner. Auch Programmsymbole und Installer-Startbild stehen unter der MIT-Lizenz. Name und
Zeichen „CivilOps“ sind Marken: Veränderte Fassungen dürfen nicht unter diesem Namen oder mit
diesem Zeichen weitergegeben werden (`assets/civilops/LICENSE-brand.txt`).

## Datenschutz

CivilOps PDF arbeitet lokal: keine Telemetrie, kein Konto, keine Verarbeitung in der Cloud.
Einzige Verbindung, die das Programm von sich aus aufbaut, ist die Update-Prüfung beim Start:
Sie fragt `download.civilops.de` nach einer neueren Fassung und nennt dabei nur Programmversion
und Programmkennung (der Server sieht wie jeder Webserver die IP-Adresse). Beim ersten Start
fragt das Programm, ob es das darf; abschaltbar unter Einstellungen, von Hand jederzeit unter
Hilfe ▸ Nach Updates suchen. Links (Website, Spenden, Links in Dokumenten) öffnen sich nur auf
Klick. Datenschutzerklärung: https://civilops.de/datenschutz.html#pdf

## Code signing policy

Die öffentlichen Windows-Fassungen sollen kostenlos über [SignPath.io](https://signpath.io)
signiert werden, mit einem Zertifikat der [SignPath Foundation](https://signpath.org)
(Antrag in Vorbereitung; bis dahin sind sie unsigniert). Signiert wird nur, was GitHub Actions in diesem
Repository aus dem veröffentlichten Quelltext baut (`.github/workflows/windows-release.yml`).

- Autor und Prüfer (Committer, Reviewer): [Checkov23](https://github.com/Checkov23)
- Freigabe jeder Signatur (Approver): [Checkov23](https://github.com/Checkov23)

Fremde Beiträge (Pull Requests) werden vor der Übernahme geprüft. Die Testfassungen des
geschützten Kanals werden lokal gebaut und nicht signiert.
