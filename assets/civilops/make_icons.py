"""CivilOps PDF: take over the app icon (PNG sizes, ICO, SVG) from the CivilOps icon set.

The CivilOps app icons (since 2026-10-08: a large picture of its own per app on the navy
gradient; for CivilOps PDF a white PDF sheet with a sand dog-ear and a dimension line) are drawn
in one place for all CivilOps apps: the project `civilops-app-symbole` (`node erzeuge.mjs`, then
`python ico.py`). Every size there is rendered on its own. This script copies the PDF set here,
under the same names:

    python make_icons.py [folder]

`folder` defaults to `civilops-app-symbole/ausgabe/pdf` next to this repository. Afterwards run
`python packaging/civilops/make_splash.py` and update the hashes in ATTRIBUTION.toml
(`cargo xtask assets` names the files whose hash changed).
"""
import shutil
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
DEFAULT = HERE.parents[2] / "civilops-app-symbole" / "ausgabe" / "pdf"
NAMES = ["civilops-pdf.svg", "civilops-pdf.ico"] + [f"civilops-pdf-{n}.png" for n in (1024, 512, 256, 128, 64, 48, 32, 24, 16)]


def main() -> int:
    source = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT
    missing = [n for n in NAMES if not (source / n).is_file()]
    if missing:
        print(f"{source}: missing {', '.join(missing)}", file=sys.stderr)
        return 1
    for n in NAMES:
        shutil.copyfile(source / n, HERE / n)
    print(f"{len(NAMES)} icon files copied from {source}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
