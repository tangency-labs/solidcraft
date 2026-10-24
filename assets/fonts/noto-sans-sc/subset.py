"""Rebuild NotoSansSC-Regular.ttf, the web app's on-demand Chinese font.

Usage (needs fontTools, `pip install fonttools`):

    python3 assets/fonts/noto-sans-sc/subset.py "NotoSansSC[wght].ttf"

The input is the unmodified variable font from google/fonts (ofl/notosanssc). It is set to
weight 400 and cut down to GB 2312, Big5 level 1, every character of the zh and zh-TW
catalogs, Latin, general punctuation, CJK punctuation, kana and full-width forms. Run it again
when a Chinese catalog gains a character the font lacks (a fonts.rs test names it).
"""

import json
import sys
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

HERE = Path(__file__).resolve().parent
LOCALES = HERE.parent.parent / "locales"
RANGES = ((0x20, 0x7E), (0xA0, 0xFF), (0x2000, 0x206F), (0x3000, 0x30FF), (0xFF00, 0xFFEF))
NAMES = (
    (1, "Noto Sans SC"),
    (2, "Regular"),
    (3, "NotoSansSC-Regular;subset"),
    (4, "Noto Sans SC Regular"),
    (6, "NotoSansSC-Regular"),
    (16, "Noto Sans SC"),
    (17, "Regular"),
)


def codec_chars(codec, leads, trails):
    chars = set()
    for lead in leads:
        for trail in trails:
            try:
                chars.update(bytes([lead, trail]).decode(codec))
            except UnicodeDecodeError:
                pass
    return chars


def wanted_chars():
    chars = codec_chars("gb2312", range(0xA1, 0xF8), range(0xA1, 0xFF))
    chars |= codec_chars("big5", range(0xA4, 0xC7), [*range(0x40, 0x7F), *range(0xA1, 0xFF)])
    for locale in ("zh", "zh-TW"):
        for message in json.loads((LOCALES / f"{locale}.json").read_text(encoding="utf-8")).values():
            chars.update(message)
    for low, high in RANGES:
        chars.update(map(chr, range(low, high + 1)))
    return chars


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    font = instancer.instantiateVariableFont(TTFont(sys.argv[1]), {"wght": 400})
    options = subset.Options()
    options.layout_features = ["*"]
    options.name_IDs = ["*"]
    options.name_languages = ["*"]
    options.notdef_outline = True
    options.hinting = False
    subsetter = subset.Subsetter(options)
    subsetter.populate(unicodes=sorted(map(ord, wanted_chars())))
    subsetter.subset(font)
    name = font["name"]
    for name_id, value in NAMES:
        name.removeNames(nameID=name_id)
        name.setName(value, name_id, 3, 1, 0x409)
    font["OS/2"].usWeightClass = 400
    # Keep the source's timestamps, so a rebuild from the same input gives the same file.
    font.recalcTimestamp = False
    font.save(HERE / "NotoSansSC-Regular.ttf")


if __name__ == "__main__":
    main()
