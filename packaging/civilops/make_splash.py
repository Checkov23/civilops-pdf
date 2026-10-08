"""CivilOps PDF: Startbild des Installers (packaging/civilops/splash.png), wie bei CivilOps CAD.

    python packaging/civilops/make_splash.py

Navy-Fläche mit runden Ecken, das Programmsymbol, der Name in Sand und „Wird installiert …“.
"""
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[2]
FONTS = ROOT / "assets" / "fonts"
W, H = 460, 330
NAVY = (22, 42, 66, 255)
SAND = (194, 168, 120, 255)
TEXT = (232, 234, 236, 255)


def centered(d: ImageDraw.ImageDraw, width: int, y: int, text: str, font: ImageFont.FreeTypeFont, fill) -> None:
    left, _, right, _ = d.textbbox((0, 0), text, font=font)
    d.text(((width - (right - left)) / 2 - left, y), text, font=font, fill=fill)


def main() -> None:
    # Viermal so groß zeichnen und verkleinern: glatte Ecken und Kanten.
    k = 4
    big = Image.new("RGBA", (W * k, H * k), (0, 0, 0, 0))
    d = ImageDraw.Draw(big)
    d.rounded_rectangle([0, 0, W * k - 1, H * k - 1], radius=22 * k, fill=NAVY)
    icon = Image.open(ROOT / "assets" / "civilops" / "civilops-pdf-1024.png").convert("RGBA").resize((176 * k, 176 * k), Image.LANCZOS)
    big.alpha_composite(icon, ((W * k - icon.width) // 2, 34 * k))
    centered(d, W * k, 222 * k, "CivilOps PDF", ImageFont.truetype(str(FONTS / "Inter-SemiBold.ttf"), 34 * k), SAND)
    centered(d, W * k, 280 * k, "Wird installiert …", ImageFont.truetype(str(FONTS / "Inter-Regular.ttf"), 15 * k), TEXT)
    out = ROOT / "packaging" / "civilops" / "splash.png"
    big.resize((W, H), Image.LANCZOS).save(out)
    print(out)


if __name__ == "__main__":
    main()
