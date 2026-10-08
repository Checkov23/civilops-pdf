"""CivilOps PDF: draw the app icon (same geometry as civilops-pdf.svg) as PNG and ICO files.

Run from this folder: python make_icons.py
"""
from PIL import Image, ImageDraw

NAVY = (0x1B, 0x3A, 0x5E, 255)
SAND = (0xC2, 0xA8, 0x78, 255)
WHITE = (255, 255, 255, 255)


def draw(size: int) -> Image.Image:
    # Drawn at 4x the 512 design grid, then scaled down for smooth edges.
    k = 4
    s = 512 * k
    im = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    d.rounded_rectangle([0, 0, s - 1, s - 1], radius=112 * k, fill=NAVY)

    def c(x, y):
        # The C mark: translate(116, 116) scale(4.375) on a 64 grid.
        return (116 + x * 4.375) * k, (116 + y * 4.375) * k

    d.polygon([c(0, 10.36), c(10.36, 0), c(64, 0), c(64, 18.29), c(18.29, 18.29), c(18.29, 45.71), c(64, 45.71), c(64, 64), c(0, 64)], fill=WHITE)
    d.rectangle([*c(32.61, 23.47), *c(32.61 + 17.07, 23.47 + 17.06)], fill=WHITE)
    # Sand badge with a sheet and a dimension line.
    d.rounded_rectangle([286 * k, 286 * k, 486 * k, 486 * k], radius=44 * k, fill=SAND)

    def p(x, y):
        return x * k, y * k

    sheet = [p(336, 312), p(408, 312), p(436, 340), p(436, 460), p(336, 460), p(336, 312)]
    d.line(sheet, fill=WHITE, width=12 * k, joint="curve")
    d.line([p(408, 312), p(408, 340), p(436, 340)], fill=WHITE, width=8 * k, joint="curve")
    for a, b in [((356, 420), (416, 420)), ((356, 404), (356, 436)), ((416, 404), (416, 436))]:
        d.line([p(*a), p(*b)], fill=WHITE, width=8 * k)
        for q in (a, b):
            r = 4 * k
            d.ellipse([q[0] * k - r, q[1] * k - r, q[0] * k + r, q[1] * k + r], fill=WHITE)
    return im.resize((size, size), Image.LANCZOS)


if __name__ == "__main__":
    big = draw(1024)
    big.save("civilops-pdf-1024.png")
    for n in (512, 256, 128, 64, 48, 32, 24, 16):
        draw(n).save(f"civilops-pdf-{n}.png")
    big.save("civilops-pdf.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    print("icons written")
