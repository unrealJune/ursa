"""Draws the app's icons (the Windows logo's constellation) at Android sizes.

    python -I scripts/make-icons.py      # from apps/android; writes assets/*.png
"""
import os
from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "assets")
TOP, BOTTOM = (56, 75, 214), (119, 59, 235)
# The constellation on the Windows logo's 256 px grid: (x, y, radius).
STARS = [(47, 87, 13), (84, 77, 11), (120, 93, 11), (148, 122, 13), (215, 128, 14), (155, 170, 12), (207, 182, 12)]
LINES = [(0, 1), (1, 2), (2, 3), (3, 4), (4, 6), (6, 5), (5, 3)]


def gradient(size):
    im = Image.new("RGB", (size, size))
    d = ImageDraw.Draw(im)
    for y in range(size):
        t = y / (size - 1)
        d.line([(0, y), (size, y)], fill=tuple(round(a + (b - a) * t) for a, b in zip(TOP, BOTTOM)))
    return im


def constellation(size, scale, color=(255, 255, 255, 255), supersample=4):
    """The stars on a transparent square, the 256 grid scaled by `scale` and centred."""
    s = size * supersample
    im = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    k = scale * s / 256
    off = (s - 256 * k) / 2 + 3 * k  # the drawing sits slightly left of centre on its grid
    pt = lambda i: (STARS[i][0] * k + off, STARS[i][1] * k + off)
    for a, b in LINES:
        d.line([pt(a), pt(b)], fill=color, width=round(4 * k))
    for i, (_, _, r) in enumerate(STARS):
        x, y = pt(i)
        d.ellipse([x - r * k, y - r * k, x + r * k, y + r * k], fill=color)
    return im.resize((size, size), Image.LANCZOS)


def main():
    os.makedirs(OUT, exist_ok=True)
    full = gradient(1024).convert("RGBA")
    full.alpha_composite(constellation(1024, 0.92))
    full.save(os.path.join(OUT, "icon.png"))
    # Adaptive icons: the launcher masks to the middle 66%, so the stars sit inside it.
    constellation(1024, 0.6).save(os.path.join(OUT, "adaptive-foreground.png"))
    gradient(1024).save(os.path.join(OUT, "adaptive-background.png"))
    constellation(1024, 0.6).save(os.path.join(OUT, "adaptive-monochrome.png"))
    constellation(512, 0.9, color=(89, 66, 225, 255)).save(os.path.join(OUT, "splash.png"))


main()
