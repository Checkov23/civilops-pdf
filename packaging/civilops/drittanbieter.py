"""Drittanbieter-Lizenzen.txt für den Programmordner von CivilOps PDF.

Die Datei enthält den Lizenztext von CivilOps PDF mit allen Urheberrechtsvermerken, die
eingebundenen Schriften, Symbole und Daten mit ihren Lizenztexten und jede Rust-Bibliothek, die in
civilops-pdf.exe steckt (normale Abhängigkeiten für Windows x64, aus `cargo metadata`), mit ihren
eigenen Lizenzdateien. Gleiche Texte stehen nur einmal da, mit der Liste der Bibliotheken.

    python packaging/civilops/drittanbieter.py <ausgabedatei>

release.py ruft `erzeugen` beim Packen auf.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

WURZEL = Path(__file__).resolve().parents[2]
ZIEL = "x86_64-pc-windows-msvc"
# Das Paket der Programmdatei civilops-pdf.exe.
PAKET = "printcraft"
LIZENZDATEI = re.compile(r"^(LICEN[CS]E|COPYING|NOTICE|COPYRIGHT|UNLICENSE)", re.I)
UNTERORDNER = {"fonts", "licenses", "license", "licences"}

# Eingebundene Schriften, Symbole und Daten: (was, Lizenzdatei im Repo).
BEIGABEN = [
    ("Lucide-Symbole (ISC)", "assets/icons/LICENSE-lucide.txt"),
    ("Schrift Inter (SIL Open Font License 1.1)", "assets/fonts/OFL-Inter.txt"),
    ("Schrift JetBrains Mono (SIL Open Font License 1.1)", "assets/fonts/OFL-JetBrainsMono.txt"),
    ("Schrift Dancing Script (SIL Open Font License 1.1)", "assets/fonts/OFL-DancingScript.txt"),
    ("Ersatzschriften für die 14 PDF-Standardschriften, Foxit/PDFium (BSD-3-Clause)", "vendor/hayro-interpret/assets/LICENSE_FOXIT"),
    ("Farbprofil CGATS001Compat-v2-micro (CC0-1.0)", "vendor/hayro-interpret/assets/CGATS_LICENSE.txt"),
]
CMAP = """CMap-Daten zur Zeichenzuordnung (über hayro-cmap):
Copyright 1990-2023 Adobe. All rights reserved. Redistributed under the BSD-3-Clause licence
(https://github.com/adobe-type-tools/cmap-resources/blob/master/LICENSE.txt). Neither the name of
Adobe nor the names of its contributors are used to endorse or promote this product."""

MIT_ALLGEMEIN = """Permission is hereby granted, free of charge, to any person obtaining a copy of this software
and associated documentation files (the "Software"), to deal in the Software without restriction,
including without limitation the rights to use, copy, modify, merge, publish, distribute,
sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or
substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT
NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES
OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE."""

BSD3 = """Redistribution and use in source and binary forms, with or without modification, are
permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this list of conditions
   and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice, this list of
   conditions and the following disclaimer in the documentation and/or other materials provided
   with the distribution.
3. Neither the name of the copyright holder nor the names of its contributors may be used to
   endorse or promote products derived from this software without specific prior written
   permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR
IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND
FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR
CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER
IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF
THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE."""

BSL = """Boost Software License - Version 1.0 - August 17th, 2003

Permission is hereby granted, free of charge, to any person or organization obtaining a copy of the
software and accompanying documentation covered by this license (the "Software") to use, reproduce,
display, distribute, execute, and transmit the Software, and to prepare derivative works of the
Software, and to permit third-parties to whom the Software is furnished to do so, all subject to
the following:

The copyright notices in the Software and this entire statement, including the above license
grant, this restriction and the following disclaimer, must be included in all copies of the
Software, in whole or in part, and all derivative works of the Software, unless such copies or
derivative works are solely in the form of machine-executable object code generated by a source
language processor.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT
NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE, TITLE AND
NON-INFRINGEMENT. IN NO EVENT SHALL THE COPYRIGHT HOLDERS OR ANYONE DISTRIBUTING THE SOFTWARE BE
LIABLE FOR ANY DAMAGES OR OTHER LIABILITY, WHETHER IN CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT
OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE."""

# Die Lizenz, unter der eine Bibliothek ohne eigene Lizenzdatei verwendet wird (bei mehreren zur
# Wahl die erste dieser Reihe).
BEVORZUGT = ["MIT", "Apache-2.0", "BSD-3-Clause", "BSL-1.0"]

STRICH = "=" * 78


def standardtext(lizenz: str) -> str:
    if lizenz == "MIT":
        return MIT_ALLGEMEIN
    if lizenz == "Apache-2.0":
        return (WURZEL / "vendor" / "egui" / "LICENSE-APACHE").read_text(encoding="utf-8").strip()
    return BSD3 if lizenz == "BSD-3-Clause" else BSL


def gewaehlt(ausdruck: str | None) -> str | None:
    teile = [t.strip(" ()") for t in re.split(r"\s+OR\s+", ausdruck or "")]
    return next((b for b in BEVORZUGT if b in teile), None)


def bibliotheken() -> list[dict]:
    """Die Pakete, die in die Programmdatei eingehen (ohne die eigenen Workspace-Pakete)."""
    roh = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--filter-platform", ZIEL, "--locked"],
        cwd=WURZEL, capture_output=True, text=True, encoding="utf-8", check=True,
    ).stdout
    md = json.loads(roh)
    nach_id = {p["id"]: p for p in md["packages"]}
    knoten = {n["id"]: n for n in md["resolve"]["nodes"]}
    eigene = set(md["workspace_members"])
    start = next(p["id"] for p in md["packages"] if p["name"] == PAKET and p["id"] in eigene)
    gesehen: set[str] = set()
    offen = [start]
    while offen:
        i = offen.pop()
        if i in gesehen:
            continue
        gesehen.add(i)
        for d in knoten[i]["deps"]:
            # Nur, was in die Programmdatei gelinkt wird (keine Test- und Build-Abhängigkeiten).
            if any(k.get("kind") is None for k in d["dep_kinds"]):
                offen.append(d["pkg"])
    return sorted((nach_id[i] for i in gesehen if i not in eigene), key=lambda p: (p["name"].lower(), p["version"]))


def lizenzdateien(paket: dict) -> list[tuple[str, str]]:
    ordner = Path(paket["manifest_path"]).parent
    dateien = [f for f in sorted(ordner.iterdir()) if f.is_file() and LIZENZDATEI.match(f.name)]
    for unter in sorted(ordner.iterdir()):
        if unter.is_dir() and unter.name.lower() in UNTERORDNER:
            dateien += [f for f in sorted(unter.iterdir()) if f.is_file() and (f.suffix.lower() == ".txt" or LIZENZDATEI.match(f.name))]
    return [(f.relative_to(ordner).as_posix(), f.read_text(encoding="utf-8", errors="replace").strip()) for f in dateien]


def erzeugen(ziel: Path) -> int:
    """Schreibt die Datei; gibt die Zahl der Bibliotheken zurück."""
    teile = [
        "CivilOps PDF: Lizenzen und Urheberrechtsvermerke",
        "",
        "CivilOps PDF, Copyright (c) 2026 CivilOps (https://civilops.de).",
        "Das Programm enthält Open-Source-Software. Ihre Urheberrechtsvermerke und Lizenztexte folgen.",
        "Name, Zeichen und Programmsymbole von CivilOps sind Marken von CivilOps und nicht Open Source.",
        "",
        STRICH,
        "1. CivilOps PDF (MIT-Lizenz)",
        "   Ausgenommen ist das Update-Modul: alle Rechte bei CivilOps.",
        STRICH,
        "",
        (WURZEL / "LICENSE-MIT").read_text(encoding="utf-8").strip(),
        "",
        STRICH,
        "2. Schriften, Symbole und Daten im Programm",
        STRICH,
    ]
    for was, datei in BEIGABEN:
        teile += ["", f"--- {was} ---", "", (WURZEL / datei).read_text(encoding="utf-8", errors="replace").strip()]
    teile += ["", "--- " + CMAP.splitlines()[0] + " ---", "", "\n".join(CMAP.splitlines()[1:])]

    pakete = bibliotheken()
    teile += ["", STRICH, f"3. Rust-Bibliotheken im Programm ({len(pakete)})", STRICH, ""]
    for p in pakete:
        quelle = p.get("repository") or p.get("homepage") or ""
        teile.append(f"{p['name']} {p['version']}  ({p.get('license') or p.get('license_file') or 'siehe Text'})  {quelle}".rstrip())

    # Gleiche Texte einmal, mit den Bibliotheken, für die sie gelten.
    texte: dict[str, list[str]] = {}
    ohne_datei: dict[str | None, list[str]] = {}
    for p in pakete:
        name = f"{p['name']} {p['version']}"
        dateien = lizenzdateien(p)
        if not dateien:
            ohne_datei.setdefault(gewaehlt(p.get("license")), []).append(f"{name} ({p.get('license') or '?'})")
            continue
        for _, text in dateien:
            texte.setdefault(text, []).append(name)
    teile += ["", STRICH, "4. Lizenztexte der Bibliotheken", STRICH]
    for text, namen in sorted(texte.items(), key=lambda kv: kv[1][0].lower()):
        teile += ["", "--- Gilt für: " + ", ".join(namen) + " ---", "", text]
    # Bibliotheken ohne eigene Lizenzdatei: der Standardtext der Lizenz, unter der sie hier stehen.
    for lizenz in BEVORZUGT:
        if lizenz in ohne_datei:
            teile += [
                "", f"--- Ohne eigene Lizenzdatei, verwendet unter {lizenz}: " + ", ".join(ohne_datei[lizenz]) + " ---", "",
                "Copyright (c) the authors of the packages listed above.", "", standardtext(lizenz),
            ]
    if None in ohne_datei:
        teile += ["", "--- Ohne eigene Lizenzdatei (Text unter https://spdx.org/licenses/) ---", "", "\n".join(ohne_datei[None])]
    ziel.write_text("\n".join(teile) + "\n", encoding="utf-8", newline="\r\n")
    return len(pakete)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    n = erzeugen(Path(sys.argv[1]))
    print(f"{sys.argv[1]}: {n} Bibliotheken")
