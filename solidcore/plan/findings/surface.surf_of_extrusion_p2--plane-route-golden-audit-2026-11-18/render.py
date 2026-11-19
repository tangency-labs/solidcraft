# SPDX-License-Identifier: MIT OR Apache-2.0
"""Plot captured degree-one intersection curves, preserving one view per case.
Usage: python3 render.py initial.json corrected-selected.json comparison.png
"""
import json
import math
from pathlib import Path
import subprocess
import sys


def segments(row):
    values = row['raw_after']['out']
    j = 2 if row['inputs']['op'] == 'global' else 1
    count = int(values[j + 1])
    j += 2
    lines = []
    for _ in range(count):
        assert values[j] == 1 and values[j + 4] == 1
        degree, size = map(int, values[j + 5:j + 7])
        assert degree == 1 and size == 2
        j += 7
        points = []
        for _ in range(size):
            x, y, z, w = values[j:j + 4]
            point = [x, y, z] if w < -1e300 else [x / w, y / w, z / w]
            assert all(math.isfinite(v) for v in point)
            points.append(point)
            j += 4
        lines.append(points)
        j += 1 + int(values[j])
    return lines


def main():
    before = {r['case']: r for r in json.loads(Path(sys.argv[1]).read_text())}
    after = {r['case']: r for r in json.loads(Path(sys.argv[2]).read_text())}
    drawing = []
    def text(x, y, label, size=17):
        assert "'" not in label
        drawing.append(f"fill '#243447' stroke none font-size {size} text {x},{y} '{label}'")
    colors = ['#b8333c', '#2169af', '#32834c', '#9757a6', '#cc751c']
    cases = [('gen0906', (-2.0, 4.0, -1.0, 1.5)),
             ('wire_line_bounded_subdomain_miss', (-1.1, 1.1, -1.2, 1.2))]
    for column, (phase, rows) in enumerate([('PR before correction', before), ('Corrected', after)]):
        xoff = column * 600
        text(xoff + 30, 35, phase, 23)
        for index, (name, bounds) in enumerate(cases):
            yoff = 60 + 370 * index
            left, right, bottom, top = bounds
            def xy(y, z):
                return xoff + 65 + (y - left) * 480 / (right - left), yoff + 270 - (z - bottom) * 215 / (top - bottom)
            def line(a, b, color, width=2):
                x0, y0 = xy(*a)
                x1, y1 = xy(*b)
                drawing.append(f"fill none stroke '{color}' stroke-width {width} line {x0:.5f},{y0:.5f} {x1:.5f},{y1:.5f}")
            text(xoff + 30, yoff + 20, name, 17)
            text(xoff + 30, yoff + 43, 'World Y/Z projection; model units', 14)
            for tick in range(math.ceil(left), math.floor(right) + 1):
                line((tick, bottom), (tick, top), '#e4e8ec', 1)
                x, y = xy(tick, bottom)
                text(round(x - 8), round(y + 22), str(tick), 13)
            for tick in range(math.ceil(bottom), math.floor(top) + 1):
                line((left, tick), (right, tick), '#e4e8ec', 1)
                x, y = xy(left, tick)
                text(round(x - 28), round(y + 5), str(tick), 13)
            line((left, bottom), (right, bottom), '#8996a3', 1)
            line((left, bottom), (left, top), '#8996a3', 1)
            if index == 1:
                # P(u,w)=(.5,u,w); the caller requests u in [.25,1], w in [-1,1].
                a, b = xy(.25, -1), xy(1, 1)
                drawing.append(f"fill '#e8f3fb' stroke '#85afd0' stroke-width 1 rectangle {a[0]:.5f},{b[1]:.5f} {b[0]:.5f},{a[1]:.5f}")
                text(xoff + 352, yoff + 95, 'Requested domain', 13)
            curves = segments(rows[name])
            for curve_index, (a, b) in enumerate(curves):
                color = colors[curve_index % len(colors)]
                # Wider first strokes keep coincident later strokes visible.
                width = max(2, 7 - curve_index * 2) if len(curves) == 2 else 3
                line((a[1], a[2]), (b[1], b[2]), color, width)
            text(xoff + 65, yoff + 324, f'{len(curves)} returned segment' + ('s' if len(curves) != 1 else ''), 17)
            if index == 0 and column == 0:
                text(xoff + 65, yoff + 347, 'Both segments cover the same 3.5-unit interval', 14)
            if index == 1 and column == 0:
                text(xoff + 65, yoff + 347, 'The returned line has u=0, outside the request', 14)
    font = Path('assets/fonts/biz-udp-gothic/BIZUDPGothic-Regular.ttf').resolve()
    subprocess.run(['magick', '-size', '1200x820', 'xc:white', '-font', str(font),
                    '-draw', '\n'.join(drawing), sys.argv[3]], check=True)


if __name__ == '__main__':
    main()
