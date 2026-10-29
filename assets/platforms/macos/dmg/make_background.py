#!/usr/bin/env python3
"""Draw the SolidCraft disk image window background (background.tiff).

A blueprint sheet: deep blue with a drafting grid, a double border with ticks, a circle round each
icon, dash-dot centre lines, a leader arrow from the app to Applications and a small title block.
Every line stops short of text. The window geometry comes from layout.json, shared with
settings.py, so the art and the icons agree.

Authoring tool only (assets/AGENTS.md): run it after editing, commit the TIFF. Packaging never
runs it. Needs Python 3.11+, Pillow and macOS `tiffutil`:

    python3 -m venv target/dmg-art && target/dmg-art/bin/pip install pillow
    target/dmg-art/bin/python assets/platforms/macos/dmg/make_background.py [--preview DIR]

All coordinates are points; the TIFF holds a 1x and a 144 dpi 2x representation.
"""
import argparse
import json
import subprocess
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

HERE = Path(__file__).resolve().parent
FONTS = HERE.parents[2] / "fonts"
LAYOUT = json.loads((HERE / "layout.json").read_text())
W, H = LAYOUT["window"]
ICON = LAYOUT["icon_size"]
AX, CY = LAYOUT["app"]
BX, _ = LAYOUT["applications"]
SS = 6  # drawing scale: 3x the 2x output, reduced afterwards

PAPER_TOP, PAPER_BOTTOM = (28, 84, 158), (18, 58, 118)
LINE = (226, 238, 252)
PALE = (190, 215, 245)
# Finder draws the icon names itself, black in Light and white in Dark appearance, and the art
# cannot change that: each name sits on a mid-tone plate that both colours read on.
PLATE = (92, 132, 196)
PLATE_W, PLATE_H, PLATE_GAP = 116, 22, 8


def px(v):
    return int(round(v * SS))


def sans(size, weight):
    # IBMPlexSans-Variable axes: wght, wdth
    f = ImageFont.truetype(str(FONTS / "ibm-plex-sans/IBMPlexSans-Variable.ttf"), px(size))
    f.set_variation_by_axes([weight, 100])
    return f


def mono(size):
    return ImageFont.truetype(str(FONTS / "ibm-plex-mono/IBMPlexMono-Regular.ttf"), px(size))


def text(d, xy, s, f, fill, anchor="la", tracking=0.0):
    """Draw text with letter spacing (points); anchor is the usual Pillow one for the whole run."""
    x, y = xy
    if tracking == 0:
        d.text((px(x), px(y)), s, font=f, fill=fill, anchor=anchor)
        return
    widths = [f.getlength(c) / SS for c in s]
    total = sum(widths) + tracking * (len(s) - 1)
    if anchor[0] == "m":
        x -= total / 2
    elif anchor[0] == "r":
        x -= total
    for c, w in zip(s, widths):
        d.text((px(x), px(y)), c, font=f, fill=fill, anchor="l" + anchor[1])
        x += w + tracking


def dashdot(d, p0, p1, fill, width=1.0, pattern=(14, 3, 2, 3)):
    """A centre line: long dash, gap, short dash, gap. Horizontal or vertical."""
    (x0, y0), (x1, y1) = p0, p1
    horiz = y0 == y1
    a, b = (x0, x1) if horiz else (y0, y1)
    pos, k = a, 0
    while pos < b:
        seg = min(pattern[k % 4], b - pos)
        if k % 2 == 0:
            q, r = ((pos, y0), (pos + seg, y0)) if horiz else ((x0, pos), (x0, pos + seg))
            d.line([(px(q[0]), px(q[1])), (px(r[0]), px(r[1]))], fill=fill, width=px(width))
        pos += seg
        k += 1


def draw_sheet():
    img = Image.new("RGB", (px(W), px(H)))
    d = ImageDraw.Draw(img)
    for y in range(px(H)):
        t = y / px(H)
        d.line([(0, y), (px(W), y)], fill=tuple(round(a + (b - a) * t) for a, b in zip(PAPER_TOP, PAPER_BOTTOM)))

    # Grid on its own layer so a soft radial mask can fade it towards the edges.
    grid = Image.new("RGBA", img.size, (0, 0, 0, 0))
    g = ImageDraw.Draw(grid)
    for i in range(0, W + 1, 12):
        g.line([(px(i), 0), (px(i), px(H))], fill=(*PALE, 95 if i % 60 == 0 else 40), width=px(0.6))
    for j in range(0, H + 1, 12):
        g.line([(0, px(j)), (px(W), px(j))], fill=(*PALE, 95 if j % 60 == 0 else 40), width=px(0.6))
    mask = Image.radial_gradient("L").resize(img.size).point(lambda v: max(0, 255 - int(v * 0.7)))
    grid.putalpha(Image.composite(grid.getchannel("A"), Image.new("L", img.size, 0), mask))
    img = Image.alpha_composite(img.convert("RGBA"), grid)
    d = ImageDraw.Draw(img)

    # Double border with a tick every 60 pt.
    d.rectangle([px(10), px(10), px(W - 10), px(H - 10)], outline=(*LINE, 255), width=px(1.8))
    d.rectangle([px(15), px(15), px(W - 15), px(H - 15)], outline=(*LINE, 110), width=px(0.6))
    for t in range(60, W, 60):
        d.line([(px(t), px(10)), (px(t), px(15))], fill=(*LINE, 255), width=px(0.8))
        d.line([(px(t), px(H - 15)), (px(t), px(H - 10))], fill=(*LINE, 255), width=px(0.8))

    # Title, top left. The left icon's centre line sits to its right and starts below it.
    text(d, (30, 24), "SolidCraft", sans(20, 600), (*LINE, 255))
    text(d, (31, 49), "PARAMETRIC 3D CAD", mono(7), (*PALE, 255), tracking=0.9)

    # A circle round each icon, with centre lines that stop short of circles, plates and text.
    r = ICON / 2 + 6
    for cx in (AX, BX):
        d.ellipse([px(cx - r), px(CY - r), px(cx + r), px(CY + r)], outline=(*LINE, 170), width=px(1.1))
    cl = (*LINE, 120)
    dashdot(d, (30, CY), (AX - r - 10, CY), cl)
    dashdot(d, (BX + r + 10, CY), (W - 30, CY), cl)
    for cx in (AX, BX):
        dashdot(d, (cx, 66), (cx, CY - r - 6), cl)
        dashdot(d, (cx, CY + r + 34), (cx, H - 42), cl)

    # Leader from the app to Applications: origin dot, line, arrowhead, a label above.
    x0, x1, y = AX + r + 8, BX - r - 8, CY + 7
    d.line([(px(x0 + 4), px(y)), (px(x1 - 10), px(y))], fill=(*LINE, 255), width=px(1.8))
    d.ellipse([px(x0 - 3), px(y - 3), px(x0 + 3), px(y + 3)], fill=(*LINE, 255))
    d.polygon([(px(x1), px(y)), (px(x1 - 11), px(y - 4.4)), (px(x1 - 11), px(y + 4.4))], fill=(*LINE, 255))
    text(d, ((x0 + x1) / 2, CY - 6), "DRAG TO INSTALL", mono(8.5), (*LINE, 255), "ms", tracking=1.4)

    # Title block, bottom right: two cells.
    bx1, by1 = W - 16, H - 16
    d.rectangle([px(bx1 - 196), px(by1 - 18), px(bx1), px(by1)], outline=(*LINE, 255), width=px(1))
    d.line([(px(bx1 - 98), px(by1 - 18)), (px(bx1 - 98), px(by1))], fill=(*LINE, 255), width=px(1))
    text(d, (bx1 - 147, by1 - 8.5), "SHEET 1/1", mono(7), (*LINE, 255), "mm", tracking=1)
    text(d, (bx1 - 49, by1 - 8.5), "SCALE 1:1", mono(7), (*LINE, 255), "mm", tracking=1)

    # Name plates where Finder draws the icon names.
    py = CY + ICON / 2 + PLATE_GAP
    for cx in (AX, BX):
        d.rounded_rectangle(
            [px(cx - PLATE_W / 2), px(py), px(cx + PLATE_W / 2), px(py + PLATE_H)], radius=px(6), fill=(*PLATE, 255)
        )

    return img.convert("RGB")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--preview", help="also write the 1x and 2x PNGs to this directory")
    ap.add_argument("--out", default=str(HERE / "background.tiff"))
    args = ap.parse_args()

    big = draw_sheet()
    with tempfile.TemporaryDirectory() as tmp:
        one, two = Path(tmp, "bg.png"), Path(tmp, "bg@2x.png")
        big.resize((W, H), Image.LANCZOS).save(one, dpi=(72, 72))
        big.resize((W * 2, H * 2), Image.LANCZOS).save(two, dpi=(144, 144))
        # A multi-representation TIFF: Finder picks the 2x one on Retina screens.
        raw = Path(tmp, "bg.tiff")
        subprocess.run(["tiffutil", "-cat", str(one), str(two), "-out", str(raw)], check=True)
        subprocess.run(["tiffutil", "-lzw", str(raw), "-out", args.out], check=True)
        if args.preview:
            out = Path(args.preview)
            out.mkdir(parents=True, exist_ok=True)
            for p in (one, two):
                (out / p.name).write_bytes(p.read_bytes())
    print("wrote", args.out)


if __name__ == "__main__":
    main()
