#!/usr/bin/env python3
"""Draw the language-menu labels of the CJK interface languages as minified SVG outlines.

The web app loads a CJK font only after its language is chosen, so the language menu cannot
write "日本語" or "한국어" as text: before the font is there the names would be boxes, and drawing
them as text would also make the app fetch a font just for opening the menu. These SVGs hold the
names as outlines instead (assets/locales/labels/*.svg, read by crates/ui-egui/src/lang_label.rs).

Each outline comes from the face its language uses in the app, unmodified, under the SIL OFL 1.1
(see ATTRIBUTION.md): Japanese from BIZ UDPGothic, Simplified Chinese, Traditional Chinese and
Korean from Noto Sans SC, TC and KR.

The SVG subset (read by lang_label.rs, absolute commands only): one `<path fill="currentColor">`
whose `d` has M L H V C Q Z with coordinates rounded to 0.1, in a viewBox 16 units high (one em).

Needs `pip install fonttools`. Run from anywhere:

    python assets/locales/labels/make_language_labels.py
"""

from pathlib import Path

from fontTools.pens.basePen import BasePen
from fontTools.ttLib import TTFont

HERE = Path(__file__).resolve().parent
FONTS = HERE.parent.parent / "fonts"

HEIGHT = 16.0
ASCENT = 0.88

LABELS = [
    ("ja", "日本語", "biz-udp-gothic/BIZUDPGothic-Regular.ttf"),
    ("ko", "한국어", "noto-sans-kr/NotoSansKR-Regular.otf"),
    ("zh", "中文", "noto-sans-sc/NotoSansSC-Regular.otf"),
    ("zh-TW", "繁體中文", "noto-sans-tc/NotoSansTC-Regular.otf"),
]


def number(value: float) -> str:
    """Shortest text of `value` at one decimal: `.5` for 0.5, `-3` for -3.0."""
    text = f"{round(value, 1):.1f}"
    if text.endswith(".0"):
        text = text[:-2]
    if text.startswith("0."):
        text = text[1:]
    elif text.startswith("-0."):
        text = "-" + text[2:]
    return "0" if text in ("-0", "") else text


def last_number(text: str) -> str:
    """The number the text ends with, with its sign."""
    start = len(text)
    while start > 0 and text[start - 1] not in " MLHVCQZ-":
        start -= 1
    if start > 0 and text[start - 1] == "-":
        start -= 1
    return text[start:]


class OutlinePen(BasePen):
    """Collect one glyph's outline as path commands in label units."""

    def __init__(self, glyphs, upem: int, left: float):
        super().__init__(glyphs)
        self.scale = HEIGHT / upem
        self.baseline = ASCENT * upem
        self.left = left
        self.commands: list[tuple[str, list[float]]] = []
        self.at = (0.0, 0.0)

    def point(self, p):
        return (round(self.left + p[0] * self.scale, 1), round((self.baseline - p[1]) * self.scale, 1))

    def _moveTo(self, p):
        self.at = self.point(p)
        self.commands.append(("M", list(self.at)))

    def _lineTo(self, p):
        q = self.point(p)
        if q == self.at:
            return
        if q[1] == self.at[1]:
            self.commands.append(("H", [q[0]]))
        elif q[0] == self.at[0]:
            self.commands.append(("V", [q[1]]))
        else:
            self.commands.append(("L", list(q)))
        self.at = q

    def _curveToOne(self, p1, p2, p3):
        a, b, c = self.point(p1), self.point(p2), self.point(p3)
        self.commands.append(("C", [*a, *b, *c]))
        self.at = c

    def _qCurveToOne(self, p1, p2):
        a, b = self.point(p1), self.point(p2)
        self.commands.append(("Q", [*a, *b]))
        self.at = b

    def _closePath(self):
        self.commands.append(("Z", []))

    _endPath = _closePath


def path_data(commands: list[tuple[str, list[float]]]) -> str:
    """Minified path text: a command letter only when it changes, a separator only where needed."""
    out = ""
    previous = ""
    for letter, values in commands:
        if letter != previous or letter in ("M", "Z"):
            out += letter
        previous = letter
        for text in (number(v) for v in values):
            if not out or out[-1].isalpha():
                out += text
            elif text.startswith("-"):
                out += text
            elif text.startswith(".") and "." in last_number(out):
                out += text
            else:
                out += " " + text
    return out


def label_svg(text: str, font_path: Path) -> str:
    font = TTFont(font_path)
    upem = font["head"].unitsPerEm
    scale = HEIGHT / upem
    cmap = font.getBestCmap()
    glyphs = font.getGlyphSet()
    hmtx = font["hmtx"]
    commands: list[tuple[str, list[float]]] = []
    pen_x = 0.0
    for char in text:
        name = cmap.get(ord(char))
        if name is None:
            raise SystemExit(f"{font_path.name} has no glyph for {char!r}")
        pen = OutlinePen(glyphs, upem, pen_x)
        glyphs[name].draw(pen)
        commands.extend(pen.commands)
        pen_x += hmtx[name][0] * scale
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {number(pen_x)} {number(HEIGHT)}">'
        f'<path fill="currentColor" d="{path_data(commands)}"/></svg>\n'
    )


def main() -> None:
    for language, text, font in LABELS:
        svg = label_svg(text, FONTS / font)
        target = HERE / f"{language}.svg"
        target.write_text(svg, encoding="utf-8", newline="\n")
        print(f"{target.name}: {len(svg.encode('utf-8'))} bytes")


if __name__ == "__main__":
    main()
