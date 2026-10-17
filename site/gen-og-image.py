#!/usr/bin/env python3
"""Generate the 1200x630 social preview image for the solidcore progress site.

Portable: Pillow only, no machine-specific paths, deterministic. Writes
site/assets/og-image.png. Run from the repo root:

    python3 site/gen-og-image.py

The card uses the site's own design tokens (site/assets/style.css) and the
repo's brand mark (assets/logo.svg): dark #141413 ground, light #f0efec text,
brand steel/ice quads (#5B6B82, #2E3B50, #E6ECF3, #C7D2E0) with the orange
#F97316 progress polyline and the muted #9c9b92 caption. Intentionally
number-free: live progress percentages go stale and preview caches hold old
images, so the card carries only logo, name, taglines, and URL.
"""
import pathlib

from PIL import Image, ImageDraw, ImageFont

OUT = pathlib.Path(__file__).resolve().parent / "assets" / "og-image.png"
W, H = 1200, 630

BG = "#141413"          # --bg (dark)
GRID = "#2c2b28"        # --grid (dark)
TEXT = "#f0efec"        # --text (dark)
MUTED = "#9c9b92"       # --muted (dark)
LINK = "#86b6ef"        # --link (dark)
GREEN = "#7fdcb6"       # --green-ink (dark)
ACCENT = "#F97316"      # logo polyline orange

# Brand quads from assets/logo.svg
STEEL = "#5B6B82"
NAVY = "#2E3B50"
ICE = "#E6ECF3"
LIGHT = "#C7D2E0"

FONT_CANDIDATES = [
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
    "C:/Windows/Fonts/arialbd.ttf",
]


def font(size, bold=False):
    name = "DejaVuSans-Bold.ttf" if bold else "DejaVuSans.ttf"
    for path in FONT_CANDIDATES:
        if path.endswith(name) and pathlib.Path(path).exists():
            return ImageFont.truetype(path, size)
    return ImageFont.load_default()


def poly(canvas, pts, fill=None, outline=None, width=1):
    canvas.polygon([(x, y) for x, y in pts], fill=fill, outline=outline, width=width)


def draw_logo(draw, cx, cy, s):
    """Reproduce the solidcore logomark (assets/logo.svg) centered at cx,cy, scale s."""
    def P(x, y):
        return (cx + x * s, cy + y * s)

    # Left quad
    poly(draw, [P(-86.60, -50.00), P(-20.78, -12.00), P(-20.78, 88.00), P(-86.60, 50.00)], fill=STEEL)
    # Right quad
    poly(draw, [P(20.78, -12.00), P(86.60, -50.00), P(86.60, 50.00), P(20.78, 88.00)], fill=NAVY)
    # Center fillet (drawn as flat quad; the SVG curve is a subtle rounding)
    poly(draw, [P(-20.78, -12.00), P(20.78, -12.00), P(20.78, 88.00), P(-20.78, 88.00)], fill=LIGHT)
    # Top quad
    poly(draw, [P(0.00, -100.00), P(86.60, -50.00), P(20.78, -12.00), P(-20.78, -12.00), P(-86.60, -50.00)], fill=ICE)
    # Progress polyline + dots
    draw.line([P(-51.96, -48.00), P(-28.58, -78.50), P(-1.73, -29.00), P(43.30, -55.00)],
              fill=ACCENT, width=max(2, int(4.2 * s)), joint="curve")
    for px, py in [(-51.96, -48.00), (-28.58, -78.50), (-1.73, -29.00), (43.30, -55.00)]:
        pt = P(px, py)
        draw.ellipse([pt[0] - 3.6 * s, pt[1] - 3.6 * s, pt[0] + 3.6 * s, pt[1] + 3.6 * s],
                     fill=ACCENT, outline=ICE, width=max(1, int(1.4 * s)))


def rrect(draw, box, radius, fill):
    draw.rounded_rectangle(box, radius=radius, fill=fill)


def main():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)

    # Subtle grid, matching --grid on dark
    for x in range(0, W, 60):
        d.line([(x, 0), (x, H)], fill=GRID, width=1)
    for y in range(0, H, 60):
        d.line([(0, y), (W, y)], fill=GRID, width=1)

    # Logo: scale so the mark spans ~300px, vertically centered-ish
    draw_logo(d, 210, 300, 2.1)

    # Text block on the right
    f_title = font(78, bold=True)
    f_sub = font(33)
    f_url = font(27)

    d.text((470, 170), "solidcore", font=f_title, fill=TEXT)
    d.text((472, 292), "Rust port of NVIDIA's SMLib B-rep kernel", font=f_sub, fill=MUTED)
    d.text((472, 338), "safe · deterministic · WASM-compatible", font=f_sub, fill=MUTED)

    # Accent rule
    d.line([(472, 476), (886, 476)], fill=ACCENT, width=3)

    # URL footer
    d.text((472, 530), "tangency-labs.github.io/solidcore", font=f_url, fill=LINK)

    OUT.parent.mkdir(parents=True, exist_ok=True)
    img.save(OUT, "PNG")
    print(f"wrote {OUT} ({img.size[0]}x{img.size[1]})")


if __name__ == "__main__":
    main()