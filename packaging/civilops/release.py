"""Baut ein Release von CivilOps PDF: Installer, tragbare Fassung, Update-Pakete und Feed (Velopack),
wie bei CivilOps CAD (dort scripts/release.py, docs/AUSLIEFERUNG.md).

    python packaging/civilops/release.py --version 0.1.2 --notizen-datei notizen.md --hochladen
    python packaging/civilops/release.py --version 0.1.2 --ohne-build --notizen "Kleinigkeiten" --hochladen
    python packaging/civilops/release.py --version 0.1.2 --lokal --ausgabe C:/tmp/feed

Vorher die Versionsnummer an zwei Stellen setzen (das Skript prüft beide und das gebaute Programm):
`apps/printcraft/Cargo.toml` (version) und `crates/engine/src/links.rs` (VERSION). Nummern laufen
fortlaufend hoch; eine vorhandene lehnt das Skript ab, Velopack spielt nur höhere ein.

Kanal `test` (Standard): Quelle ist der geschützte Ordner `https://download.civilops.de/pdf-test`.
Benutzer und Passwort stehen in `../civilops-pdf-releases/zugang-test.txt` (Zeilen `benutzer = …`,
`passwort = …`, nie ins Repo) und landen im Paket (`aktualisierung.txt`), damit sich der Updater
anmelden kann. `--lokal`: Quelle ist ein Ordner auf diesem Rechner, ohne Anmeldung (zum Testen).

Kanal `stabil` (öffentlich, Freeware): Quelle ist `https://download.civilops.de/pdf`, ohne
Anmeldung und ohne Zugang im Paket. Hochladen nur mit `--oeffentlich`, denn danach kann jeder das
Programm herunterladen und installierte Programme holen es sich:

    python packaging/civilops/release.py --version 1.0.0 --kanal stabil --notizen-datei n.md --hochladen --oeffentlich

Ein installiertes Programm bleibt in seinem Kanal (Test-Installationen holen weiter aus pdf-test).

Ergebnis im Ausgabeordner (Standard `../civilops-pdf-releases/web/<kanal>`): Installer
`CivilOps-PDF-<version>-Setup.exe`, tragbare Fassung `CivilOps-PDF-<version>-Portable.zip` (beide
auch ohne Nummer), Voll- und Unterschiedspakete, Feed `releases.<kanal>.json`. `--hochladen`
kopiert die neuen Dateien der Reihe nach auf den Webspace, die Versionslisten zuletzt, damit kein
Programm eine Version sieht, deren Paket noch fehlt.

Signatur: Umgebungsvariable `CIVILOPS_SIGNATUR` mit signtool-Parametern. Ohne sie bleibt das Paket
unsigniert (Windows zeigt dann „Unbekannter Herausgeber“).
"""
import argparse
import hashlib
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

WURZEL = Path(__file__).resolve().parents[2]
RELEASES = WURZEL.parent / "civilops-pdf-releases"
VPK = Path(os.environ.get(
    "VPK_DLL",
    WURZEL.parent / "civilops-cad-werkzeuge" / "vpk" / "1.2.158" / "tools" / "net8.0" / "any" / "vpk.dll",
))
SEMVER = re.compile(r"^\d+\.\d+\.\d+$")

PAKET_ID = "CivilOpsPDF"  # Installationsordner %LOCALAPPDATA%\CivilOpsPDF, nie ändern
TITEL = "CivilOps PDF"
EXE = "civilops-pdf.exe"
NAME = "CivilOps-PDF"  # Dateinamen für Download und GitHub-Release
# Lizenzen und Urheberrechtsvermerke im Programmordner (drittanbieter.py erzeugt die Datei).
LIZENZEN = "Drittanbieter-Lizenzen.txt"

WEB = "https://download.civilops.de"
# Wohin hochgeladen wird, steht nicht im Repo: `../civilops-pdf-releases/server.txt` mit den Zeilen
# `ziel = benutzer@host` und `schluessel = <Pfad zum SSH-Schlüssel>` (oder CIVILOPS_SSH_ZIEL und
# CIVILOPS_SSH_SCHLUESSEL in der Umgebung).
SERVER_DATEI = WURZEL.parent / "civilops-pdf-releases" / "server.txt"
SERVER_ORDNER = "civilops-download"
# Öffentlicher Kanal: eigener Ordner ohne Anmeldung.
OEFFENTLICH = "stabil"


def lauf(befehl: list, cwd: Path) -> None:
    print(">", " ".join(str(b) for b in befehl))
    r = subprocess.run([str(b) for b in befehl], cwd=cwd)
    if r.returncode != 0:
        sys.exit(f"Abbruch: {befehl[0]} endete mit {r.returncode}")


def version_im_code() -> tuple[str, str]:
    cargo = (WURZEL / "apps" / "printcraft" / "Cargo.toml").read_text(encoding="utf-8")
    links = (WURZEL / "crates" / "engine" / "src" / "links.rs").read_text(encoding="utf-8")
    a = re.search(r'^version = "([^"]+)"', cargo, re.M)
    b = re.search(r'pub const VERSION: &str = "([^"]+)";', links)
    return (a.group(1) if a else "?", b.group(1) if b else "?")


def zugang_lesen(datei: Path) -> str:
    werte = {}
    for zeile in datei.read_text(encoding="utf-8").splitlines():
        if "=" in zeile and not zeile.lstrip().startswith("#"):
            k, v = zeile.split("=", 1)
            werte[k.strip()] = v.strip()
    if not werte.get("benutzer") or not werte.get("passwort"):
        sys.exit(f"{datei}: benutzer und passwort fehlen")
    return f"{werte['benutzer']}:{werte['passwort']}"


def server() -> tuple[str, str]:
    """SSH-Ziel und Schlüssel fürs Hochladen (Umgebung vor `server.txt`)."""
    werte = {}
    if SERVER_DATEI.is_file():
        for zeile in SERVER_DATEI.read_text(encoding="utf-8").splitlines():
            if "=" in zeile and not zeile.lstrip().startswith("#"):
                k, v = zeile.split("=", 1)
                werte[k.strip()] = v.strip()
    ziel = os.environ.get("CIVILOPS_SSH_ZIEL") or werte.get("ziel")
    schluessel = os.environ.get("CIVILOPS_SSH_SCHLUESSEL") or werte.get("schluessel")
    if not ziel or not schluessel:
        sys.exit(f"Server fehlt: {SERVER_DATEI} (ziel = benutzer@host, schluessel = …) oder CIVILOPS_SSH_ZIEL/CIVILOPS_SSH_SCHLUESSEL")
    return ziel, schluessel


def md5(datei: Path) -> str:
    return hashlib.md5(datei.read_bytes()).hexdigest()


def hochladen(ausgabe: Path, dateien: list[Path], server_pfad: str) -> None:
    """Dateien der Reihe nach hochladen (IONOS trennt parallele Verbindungen und manchmal auch
    eine einzelne: jede Datei bis zu viermal versuchen)."""
    ziel, schluessel = server()
    optionen = ["-4", "-o", "BatchMode=yes", "-o", "ConnectTimeout=20", "-i", schluessel]
    lauf(["ssh", *optionen, ziel, f"mkdir -p ~/{server_pfad}"], ausgabe)
    for f in dateien:
        befehl = ["scp", *optionen, f.name, f"{ziel}:{server_pfad}/{f.name}"]
        for versuch in range(1, 5):
            print(">", " ".join(str(b) for b in befehl))
            if subprocess.run([str(b) for b in befehl], cwd=ausgabe, stdin=subprocess.DEVNULL).returncode == 0:
                break
            if versuch == 4:
                sys.exit(f"Abbruch: scp für {f.name} viermal fehlgeschlagen")
            print(f"  scp fehlgeschlagen (Versuch {versuch}), neuer Versuch in 10 s")
            time.sleep(10)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--version", required=True, help="Versionsnummer, z. B. 0.1.2")
    ap.add_argument("--kanal", default="test", help="Update-Kanal (test, später stabil)")
    ap.add_argument("--lokal", action="store_true", help="Feed als Ordner auf diesem Rechner (ohne Web)")
    ap.add_argument("--ausgabe", help="Ausgabeordner")
    ap.add_argument("--quelle", help="Update-Quelle im Paket (Ordner oder https-Adresse)")
    ap.add_argument("--zugang-datei", help="Datei mit benutzer/passwort für die Quelle")
    ap.add_argument("--notizen", help="Versionshinweise (Text)")
    ap.add_argument("--notizen-datei", help="Versionshinweise aus einer Markdown-Datei")
    ap.add_argument("--ohne-build", action="store_true", help="vorhandenen Build verpacken")
    ap.add_argument("--hochladen", action="store_true", help="neue Dateien auf download.civilops.de kopieren")
    ap.add_argument("--oeffentlich", action="store_true", help="bestätigt das Hochladen in den öffentlichen Kanal stabil")
    a = ap.parse_args()

    if not SEMVER.match(a.version):
        sys.exit(f"Versionsnummer {a.version!r} passt nicht (z. B. 0.1.2)")
    if not re.fullmatch(r"[a-z0-9]+", a.kanal):
        sys.exit("Kanal nur aus Kleinbuchstaben und Ziffern, z. B. test")
    if a.kanal not in ("test", OEFFENTLICH):
        sys.exit(f"Kanal {a.kanal!r}: nur test oder {OEFFENTLICH}")
    if a.kanal == OEFFENTLICH and a.hochladen and not a.oeffentlich:
        sys.exit("Kanal stabil ist öffentlich: zum Hochladen zusätzlich --oeffentlich angeben")
    if not VPK.exists():
        sys.exit(f"vpk fehlt: {VPK} (NuGet-Paket vpk 1.2.158, siehe civilops-cad docs/AUSLIEFERUNG.md)")
    im_code = version_im_code()
    if im_code != (a.version, a.version):
        sys.exit(f"Version im Code {im_code} (Cargo.toml, links.rs) passt nicht zu {a.version}: erst beide setzen")

    zugang = None
    if a.lokal:
        ausgabe = Path(a.ausgabe) if a.ausgabe else RELEASES / "lokal" / a.kanal
        quelle = a.quelle or str(Path(ausgabe).resolve())
        server_pfad = None
    elif a.kanal == OEFFENTLICH:
        # Öffentlich: kein Zugang im Paket, eigener Ordner ohne Anmeldung.
        ausgabe = Path(a.ausgabe) if a.ausgabe else RELEASES / "web" / a.kanal
        quelle = a.quelle or f"{WEB}/pdf"
        server_pfad = f"{SERVER_ORDNER}/pdf"
    else:
        ausgabe = Path(a.ausgabe) if a.ausgabe else RELEASES / "web" / a.kanal
        quelle = a.quelle or f"{WEB}/pdf-{a.kanal}"
        server_pfad = f"{SERVER_ORDNER}/pdf-{a.kanal}"
        datei = Path(a.zugang_datei) if a.zugang_datei else RELEASES / f"zugang-{a.kanal}.txt"
        if not datei.exists():
            sys.exit(f"Zugang fehlt: {datei} (benutzer = …, passwort = …)")
        zugang = zugang_lesen(datei)
    ausgabe = ausgabe.resolve()
    ausgabe.mkdir(parents=True, exist_ok=True)
    if any(ausgabe.glob(f"{PAKET_ID}-{a.version}-*full.nupkg")):
        sys.exit(f"Version {a.version} gibt es im Kanal {a.kanal} schon; neue Nummer wählen")

    if not a.ohne_build:
        lauf(["cargo", "build", "--release", "-p", "printcraft"], WURZEL)
    exe = WURZEL / "target" / "release" / EXE
    gemeldet = subprocess.run([str(exe), "--version"], capture_output=True, text=True).stdout.strip()
    if gemeldet != f"civilops-pdf {a.version}":
        sys.exit(f"Das gebaute Programm meldet {gemeldet!r} statt {a.version}: neu bauen (ohne --ohne-build)")

    # Paketinhalt: Programm, Anleitung, Lizenzen, Update-Quelle
    paket = ausgabe.parent / "_paket" / f"{a.version}-{a.kanal}"
    if paket.exists():
        shutil.rmtree(paket)
    paket.mkdir(parents=True)
    shutil.copy2(exe, paket / EXE)
    shutil.copy2(WURZEL / "packaging" / "civilops" / "LIESMICH.txt", paket / "LIESMICH.txt")
    import drittanbieter  # liegt neben diesem Skript

    print(f"{LIZENZEN}: {drittanbieter.erzeugen(paket / LIZENZEN)} Bibliotheken")
    zeilen = ["# Update-Quelle von CivilOps PDF (vom Release-Skript geschrieben)", f"kanal = {a.kanal}", f"quelle = {quelle}"]
    if zugang:
        zeilen.append(f"zugang = {zugang}")
    (paket / "aktualisierung.txt").write_text("\n".join(zeilen) + "\n", encoding="utf-8")

    befehl = [
        "dotnet", VPK, "pack", "--skip-updates", "--yes",
        "--packId", PAKET_ID,
        "--packVersion", a.version,
        "--packDir", paket,
        "--mainExe", EXE,
        "--packTitle", TITEL,
        "--packAuthors", "CivilOps",
        "--icon", WURZEL / "assets" / "civilops" / "civilops-pdf.ico",
        "--splashImage", WURZEL / "packaging" / "civilops" / "splash.png",
        "--splashProgressColor", "#C2A878",
        "--runtime", "win-x64",
        "--channel", a.kanal,
        "--outputDir", ausgabe,
    ]
    notizen = None
    if a.notizen_datei:
        notizen = Path(a.notizen_datei).read_text(encoding="utf-8")
    elif a.notizen:
        notizen = a.notizen
    if notizen:
        datei = paket.parent / f"notizen-{a.version}-{a.kanal}.md"
        datei.write_text(notizen.strip() + "\n", encoding="utf-8")
        befehl += ["--releaseNotes", datei]
    signatur = os.environ.get("CIVILOPS_SIGNATUR")
    if signatur:
        befehl += ["--signParams", signatur]
    else:
        print("Hinweis: ohne Signatur (CIVILOPS_SIGNATUR nicht gesetzt)")
    lauf(befehl, WURZEL)

    setup = ausgabe / f"{PAKET_ID}-{a.kanal}-Setup.exe"
    tragbar = ausgabe / f"{PAKET_ID}-{a.kanal}-Portable.zip"
    kopien = []
    for quelle_datei, ziel in [
        (setup, f"{NAME}-{a.version}-Setup.exe"),
        (tragbar, f"{NAME}-{a.version}-Portable.zip"),
        (setup, f"{NAME}-Setup.exe"),
        (tragbar, f"{NAME}-Portable.zip"),
    ]:
        if quelle_datei.exists():
            shutil.copy2(quelle_datei, ausgabe / ziel)
            kopien.append(ausgabe / ziel)
    print(f"Quelle:    {quelle}" + ("  (mit Anmeldung)" if zugang else ""))
    for f in sorted(ausgabe.iterdir()):
        if f.is_file():
            print(f"  {f.name:48s} {f.stat().st_size / 1e6:8.1f} MB")
    for f in kopien[:2]:
        print(f"md5 {md5(f)}  {f.name}")

    if a.hochladen:
        if server_pfad is None:
            sys.exit("--hochladen geht nur ohne --lokal")
        pakete = [ausgabe / f"{PAKET_ID}-{a.version}-{a.kanal}-full.nupkg", ausgabe / f"{PAKET_ID}-{a.version}-{a.kanal}-delta.nupkg"]
        pakete += kopien[:1] + kopien[2:]  # Installer mit Nummer, dazu Installer und tragbare Fassung ohne Nummer
        # Versionslisten zuletzt: erst dann sehen installierte Programme die neue Fassung.
        listen = [ausgabe / f"assets.{a.kanal}.json", ausgabe / f"RELEASES-{a.kanal}", ausgabe / f"releases.{a.kanal}.json"]
        hochladen(ausgabe, [f for f in pakete + listen if f.exists()], server_pfad)
        print(f"Hochgeladen nach {WEB}/{server_pfad.removeprefix(SERVER_ORDNER + '/')}/")
    return 0


if __name__ == "__main__":
    sys.exit(main())
