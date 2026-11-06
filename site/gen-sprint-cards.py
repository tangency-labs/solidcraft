#!/usr/bin/env python3
"""Generate each sprint's 1200x630 social preview card for the progress site.

Portable: Pillow and the standard library only, no machine-specific paths, deterministic.
Reads site/sprints.toml and writes site/assets/sprints/<slug>-card.png beside each
emblem. Run from the repo root:

    python3 site/gen-sprint-cards.py

The card matches site/gen-og-image.py's: dark #141413 ground with its #2c2b28 grid,
light #f0efec title, muted #9c9b92 summary, the orange #F97316 rule and the #86b6ef URL
footer. The sprint's emblem stands on the left in place of the logomark.
"""
import pathlib
import textwrap
import tomllib

from PIL import Image, ImageDraw, ImageFont

SITE = pathlib.Path(__file__).resolve().parent
W, H = 1200, 630

BG = "#141413"          # --bg (dark)
GRID = "#2c2b28"        # --grid (dark)
TEXT = "#f0efec"        # --text (dark)
MUTED = "#9c9b92"       # --muted (dark)
LINK = "#86b6ef"        # --link (dark)
ACCENT = "#F97316"      # logo polyline orange

FONT_CANDIDATES = [
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    str(pathlib.Path.home() / "Library/Fonts/DejaVuSans-Bold.ttf"),
    str(pathlib.Path.home() / "Library/Fonts/DejaVuSans.ttf"),
    "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
    "C:/Windows/Fonts/arialbd.ttf",
]


def font(size, bold=False):
    name = "DejaVuSans-Bold.ttf" if bold else "DejaVuSans.ttf"
    for path in FONT_CANDIDATES:
        if path.endswith(name) and pathlib.Path(path).exists():
            return ImageFont.truetype(path, size)
    return ImageFont.load_default()


def card(sprint):
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    for x in range(0, W, 60):
        d.line([(x, 0), (x, H)], fill=GRID, width=1)
    for y in range(0, H, 60):
        d.line([(0, y), (W, y)], fill=GRID, width=1)

    # The emblem, scaled to 500 px tall, centred on the left third.
    emblem = Image.open(SITE / "assets/sprints" / sprint["emblem"]).convert("RGBA")
    scale = 500 / emblem.height
    emblem = emblem.resize((round(emblem.width * scale), 500), Image.LANCZOS)
    img.paste(emblem, (235 - emblem.width // 2, (H - emblem.height) // 2), emblem)

    x = 480
    d.text((x, 120), sprint["title"], font=font(64, bold=True), fill=TEXT)
    f_sum = font(28)
    y = 224
    for line in textwrap.wrap(sprint["summary"], width=44)[:6]:
        d.text((x + 2, y), line, font=f_sum, fill=MUTED)
        y += 40
    d.line([(x + 2, 494), (x + 414, 494)], fill=ACCENT, width=3)
    d.text((x + 2, 530), f"tangency-labs.github.io/solidcore/sprints/{sprint['slug']}/",
           font=font(24), fill=LINK)

    out = SITE / "assets/sprints" / f"{sprint['slug']}-card.png"
    img.save(out, "PNG")
    print(f"wrote {out} ({img.size[0]}x{img.size[1]})")


def main():
    for sprint in tomllib.loads((SITE / "sprints.toml").read_text())["sprint"]:
        card(sprint)


if __name__ == "__main__":
    main()
