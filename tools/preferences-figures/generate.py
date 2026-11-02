#!/usr/bin/env python3
"""Compile docs/design/preferences/figures/*.svg into ui_kit/preferences_figure_data.rs.

Each figure becomes a list of drawing operations (rectangles, circles, flattened polygons, text) in
the figure's own coordinates, with colours as roles (`Ink`) instead of hex values. A role is the
light/dark colour pair the board used at that position; docs/design/preferences/tokens.md says which
`theme::Tokens` field each role reads. Run from the repository root:

    python3 tools/preferences-figures/generate.py
"""
import collections
import json
import math
import os
import re
import sys
import xml.etree.ElementTree as ET

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
FIGS = os.path.join(ROOT, 'docs/design/preferences/figures')
OUT = os.path.join(ROOT, 'crates/ui-egui/src/ui_kit/preferences_figure_data.rs')

# Skipped: the Orientation figures already exist in ui_kit and stay as they are.
SKIP = {'orientation-z-up', 'orientation-y-up', 'style-shaded-edges'}
THEMES = ['theme-light', 'theme-dark', 'theme-gruvbox', 'theme-imported']

PAIRS = {
    ('#F7F8FA', '#1C1F26'): 'Ground',
    ('#6F7C8F', '#A3AEC2'): 'Line',
    ('#236EAD', '#8CBEF5'): 'Accent',
    ('#FFFFFF', '#6B7790'): 'Top',
    ('#4A5868', '#B4BBC8'): 'Dim',
    ('#E4E9F0', '#515B72'): 'Left',
    ('#C9D2DE', '#394256'): 'Right',
    ('#D5DBE3', '#3F4552'): 'Border',
    ('#C3CBD6', '#4A5262'): 'ControlBorder',
    ('#FFFFFF', '#2F343E'): 'Card',
    ('#1B2430', '#DEE2E8'): 'Text',
    ('#8A5300', '#E8C24A'): 'Reference',
    ('#2F6FD0', '#6CA8F7'): 'AxisZ',
    ('#D64545', '#F27A7A'): 'AxisX',
    ('#2E9E5B', '#62D48E'): 'AxisY',
    ('#FFFFFF', '#FFFFFF'): 'White',
    ('#B84A00', '#F0A27A'): 'Note',
    ('#FBEBDD', '#4A3326'): 'NoteFill',
    ('#2F7D5B', '#7FD1AE'): 'Ok',
    ('#C98A00', '#E8C24A'): 'Warn',
    ('#FFFFFF', '#1C1F26'): 'Control',
    ('#0072B2', '#56B4E9'): 'Cv0',
    ('#E69F00', '#F0C060'): 'Cv1',
    ('#E69F00', '#E69F00'): 'Cv1',
    ('#D55E00', '#F0A27A'): 'Cv2',
    ('#56B4E9', '#8CBEF5'): 'Cv3',
    ('#56B4E9', '#56B4E9'): 'Cv3',
    ('#CC79A7', '#CC79A7'): 'Cv4',
}


def num(v, d=0.0):
    return d if v is None else float(re.match(r'-?[\d.]+(?:e-?\d+)?', v.strip()).group(0))


def parse_color(v):
    """(hex6 upper, alpha) or None for none/url."""
    if v is None or v == 'none':
        return None
    v = v.strip()
    if v.startswith('url('):
        return ('url', re.search(r'#([\w-]+)', v).group(1))
    m = re.fullmatch(r'#([0-9A-Fa-f]{3,8})', v)
    if not m:
        raise ValueError(v)
    h = m.group(1)
    if len(h) == 3:
        h = ''.join(c * 2 for c in h)
    a = 1.0
    if len(h) == 8:
        a = int(h[6:], 16) / 255.0
        h = h[:6]
    return ('#' + h.upper(), a)


def mat_mul(a, b):
    return (a[0] * b[0] + a[2] * b[1], a[1] * b[0] + a[3] * b[1], a[0] * b[2] + a[2] * b[3], a[1] * b[2] + a[3] * b[3],
            a[0] * b[4] + a[2] * b[5] + a[4], a[1] * b[4] + a[3] * b[5] + a[5])


def parse_transform(s):
    m = (1, 0, 0, 1, 0, 0)
    for name, args in re.findall(r'(\w+)\(([^)]*)\)', s or ''):
        v = [float(x) for x in re.split(r'[ ,]+', args.strip())]
        if name == 'translate':
            t = (1, 0, 0, 1, v[0], v[1] if len(v) > 1 else 0)
        elif name == 'scale':
            t = (v[0], 0, 0, v[1] if len(v) > 1 else v[0], 0, 0)
        elif name == 'matrix':
            t = tuple(v)
        else:
            raise ValueError(name)
        m = mat_mul(m, t)
    return m


def ap(m, x, y):
    return (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])


def flatten_path(d, m):
    toks = re.findall(r'[MmLlHhVvZzAaQqCc]|-?[\d.]+(?:e-?\d+)?', d)
    i = 0
    subs = []
    cur = None
    x = y = sx = sy = 0.0
    cmd = None

    def nxt():
        nonlocal i
        v = float(toks[i])
        i += 1
        return v

    def add(px, py):
        cur.append(ap(m, px, py))

    while i < len(toks):
        if re.fullmatch(r'[A-Za-z]', toks[i]):
            cmd = toks[i]
            i += 1
            if cmd in 'Zz':
                if cur:
                    subs.append((cur, True))
                    cur = None
                x, y = sx, sy
                continue
        c = cmd
        if c in 'Mm':
            if cur and len(cur) > 0:
                subs.append((cur, False))
            nx, ny = nxt(), nxt()
            if c == 'm':
                nx, ny = x + nx, y + ny
            x, y = sx, sy = nx, ny
            cur = []
            add(x, y)
            cmd = 'L' if c == 'M' else 'l'
        elif c in 'Ll':
            nx, ny = nxt(), nxt()
            if c == 'l':
                nx, ny = x + nx, y + ny
            x, y = nx, ny
            if cur is None:
                cur = []
                add(sx, sy)
            add(x, y)
        elif c in 'Hh':
            nx = nxt()
            x = x + nx if c == 'h' else nx
            if cur is None:
                cur = []
                add(sx, sy)
            add(x, y)
        elif c in 'Vv':
            ny = nxt()
            y = y + ny if c == 'v' else ny
            if cur is None:
                cur = []
                add(sx, sy)
            add(x, y)
        elif c in 'Qq':
            a, b, e, f = nxt(), nxt(), nxt(), nxt()
            if c == 'q':
                a, b, e, f = x + a, y + b, x + e, y + f
            for k in range(1, 13):
                t = k / 12
                add((1 - t) ** 2 * x + 2 * (1 - t) * t * a + t * t * e, (1 - t) ** 2 * y + 2 * (1 - t) * t * b + t * t * f)
            x, y = e, f
        elif c in 'Cc':
            a, b, e, f, g, h = (nxt() for _ in range(6))
            if c == 'c':
                a, b, e, f, g, h = x + a, y + b, x + e, y + f, x + g, y + h
            for k in range(1, 17):
                t = k / 16
                u = 1 - t
                add(u ** 3 * x + 3 * u * u * t * a + 3 * u * t * t * e + t ** 3 * g, u ** 3 * y + 3 * u * u * t * b + 3 * u * t * t * f + t ** 3 * h)
            x, y = g, h
        elif c in 'Aa':
            rx, ry, rot, la, sw, ex, ey = (nxt() for _ in range(7))
            if c == 'a':
                ex, ey = x + ex, y + ey
            pts = arc(x, y, rx, ry, rot, int(la), int(sw), ex, ey)
            for px, py in pts:
                add(px, py)
            x, y = ex, ey
        else:
            raise ValueError(c)
    if cur:
        subs.append((cur, False))
    return subs


def arc(x1, y1, rx, ry, phi, fa, fs, x2, y2):
    if rx == 0 or ry == 0:
        return [(x2, y2)]
    ph = math.radians(phi)
    cp, sp = math.cos(ph), math.sin(ph)
    dx, dy = (x1 - x2) / 2, (y1 - y2) / 2
    x1p, y1p = cp * dx + sp * dy, -sp * dx + cp * dy
    rx, ry = abs(rx), abs(ry)
    lam = x1p ** 2 / rx ** 2 + y1p ** 2 / ry ** 2
    if lam > 1:
        rx *= math.sqrt(lam)
        ry *= math.sqrt(lam)
    num_ = rx ** 2 * ry ** 2 - rx ** 2 * y1p ** 2 - ry ** 2 * x1p ** 2
    den = rx ** 2 * y1p ** 2 + ry ** 2 * x1p ** 2
    co = math.sqrt(max(0, num_ / den)) * (-1 if fa == fs else 1)
    cxp, cyp = co * rx * y1p / ry, -co * ry * x1p / rx
    cx, cy = cp * cxp - sp * cyp + (x1 + x2) / 2, sp * cxp + cp * cyp + (y1 + y2) / 2

    def ang(ux, uy, vx, vy):
        a = math.atan2(ux * vy - uy * vx, ux * vx + uy * vy)
        return a

    th1 = ang(1, 0, (x1p - cxp) / rx, (y1p - cyp) / ry)
    dth = ang((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry)
    if not fs and dth > 0:
        dth -= 2 * math.pi
    if fs and dth < 0:
        dth += 2 * math.pi
    n = max(6, int(abs(dth) / (math.pi / 16)))
    out = []
    for k in range(1, n + 1):
        t = th1 + dth * k / n
        out.append((cp * rx * math.cos(t) - sp * ry * math.sin(t) + cx, sp * rx * math.cos(t) + cp * ry * math.sin(t) + cy))
    return out


def rr_poly(x, y, w, h, r, m, seg=6):
    pts = []
    if r <= 0:
        for px, py in ((x, y), (x + w, y), (x + w, y + h), (x, y + h)):
            pts.append(ap(m, px, py))
        return pts
    r = min(r, w / 2, h / 2)
    for cx, cy, a0 in ((x + w - r, y + r, -90), (x + w - r, y + h - r, 0), (x + r, y + h - r, 90), (x + r, y + r, 180)):
        for k in range(seg + 1):
            a = math.radians(a0 + 90 * k / seg)
            pts.append(ap(m, cx + r * math.cos(a), cy + r * math.sin(a)))
    return pts


class Walker:
    """Walks one SVG into a flat op list; colours stay as hex until paired."""

    def __init__(self, root):
        self.defs = {}
        self.clips = {}
        self.ops = []
        self.root = root

    @staticmethod
    def tag(e):
        return e.tag.split('}')[-1]

    def collect(self, e):
        for c in e:
            t = self.tag(c)
            if t in ('linearGradient', 'radialGradient'):
                self.defs[c.get('id')] = c
            elif t == 'clipPath':
                self.clips[c.get('id')] = c
            elif t == 'defs':
                self.collect(c)
            else:
                self.collect(c)

    def run(self):
        self.collect(self.root)
        vb = [float(v) for v in self.root.get('viewBox').split()]
        self.size = (vb[2], vb[3])
        self.walk(self.root, (1, 0, 0, 1, -vb[0], -vb[1]), None, root=True)
        return self.ops

    def style(self, e, mscale):
        f = parse_color(e.get('fill', '#000000'))
        s = parse_color(e.get('stroke'))
        fo = num(e.get('fill-opacity'), 1.0) * num(e.get('opacity'), 1.0)
        so = num(e.get('stroke-opacity'), 1.0) * num(e.get('opacity'), 1.0)
        sw = num(e.get('stroke-width'), 1.0) * mscale if s else 0
        dash = None
        if e.get('stroke-dasharray'):
            dv = [float(v) for v in re.split(r'[ ,]+', e.get('stroke-dasharray').strip())]
            if len(dv) >= 2 and dv[1] > 0:
                dash = (dv[0] * mscale, dv[1] * mscale)
        round_cap = e.get('stroke-linecap') == 'round'
        return f, fo, s, so, sw, dash, round_cap

    def walk(self, e, m, clip, root=False):
        for c in e:
            t = self.tag(c)
            if t in ('defs', 'linearGradient', 'radialGradient', 'clipPath', 'stop'):
                continue
            mc = m
            if t == 'svg':
                x, y = num(c.get('x'), 0), num(c.get('y'), 0)
                w, h = num(c.get('width')), num(c.get('height'))
                vb = [float(v) for v in c.get('viewBox').split()]
                mc = mat_mul(m, (1, 0, 0, 1, x, y))
                mc = mat_mul(mc, (w / vb[2], 0, 0, h / vb[3], -vb[0] * w / vb[2], -vb[1] * h / vb[3]))
                self.collect_clips(c)
                # A nested svg clips its content to its own viewport.
                self.ops.append(('clip', rr_poly(x, y, w, h, 0, m)))
                self.walk(c, mc, clip)
                self.ops.append(('unclip',))
                continue
            if t == 'g':
                mc = mat_mul(m, parse_transform(c.get('transform')))
                cl = clip
                if c.get('clip-path'):
                    cid = re.search(r'#([\w-]+)', c.get('clip-path')).group(1)
                    cp = self.clips[cid]
                    pts = self.clip_points(cp, mc)
                    self.ops.append(('clip', pts))
                    self.walk(c, mc, cl)
                    self.ops.append(('unclip',))
                else:
                    self.walk(c, mc, cl)
                continue
            if c.get('transform'):
                mc = mat_mul(m, parse_transform(c.get('transform')))
            det = abs(mc[0] * mc[3] - mc[1] * mc[2])
            ms = math.sqrt(det)
            f, fo, s, so, sw, dash, rc = self.style(c, ms)
            axis = abs(mc[1]) < 1e-9 and abs(mc[2]) < 1e-9
            if t == 'rect':
                x, y, w, h = num(c.get('x'), 0), num(c.get('y'), 0), num(c.get('width')), num(c.get('height'))
                r = num(c.get('rx'), 0)
                if f and f[0] == 'url' and f[1] in self.defs:
                    g = self.defs[f[1]]
                    stops = [(num(st.get('offset')), parse_color(st.get('stop-color'))[0]) for st in g]
                    self.ops.append(('poly', rr_poly(x, y, w, h, r, mc), True, ('grad', stops[0][1], stops[-1][1], ap(mc, 0, y)[1], ap(mc, 0, y + h)[1]), fo, s, so, sw, dash, rc))
                elif axis and mc[0] > 0 and mc[3] > 0:
                    p0 = ap(mc, x, y)
                    self.ops.append(('rect', p0[0], p0[1], w * mc[0], h * mc[3], r * mc[0], f, fo, s, so, sw, dash))
                else:
                    self.ops.append(('poly', rr_poly(x, y, w, h, r, mc), True, f and ('solid', f), fo, s, so, sw, dash, rc))
            elif t == 'circle':
                cx, cy, r = num(c.get('cx')), num(c.get('cy')), num(c.get('r'))
                if abs(mc[1]) < 1e-9 and abs(mc[2]) < 1e-9 and abs(mc[0] - mc[3]) < 1e-9:
                    p0 = ap(mc, cx, cy)
                    self.ops.append(('circle', p0[0], p0[1], r * mc[0], f, fo, s, so, sw, dash))
                else:
                    pts = [ap(mc, cx + r * math.cos(a * math.pi / 12), cy + r * math.sin(a * math.pi / 12)) for a in range(24)]
                    self.ops.append(('poly', pts, True, f and ('solid', f), fo, s, so, sw, dash, rc))
            elif t == 'ellipse':
                cx, cy, rx, ry = num(c.get('cx')), num(c.get('cy')), num(c.get('rx')), num(c.get('ry'))
                if f and f[0] == 'url':
                    g = self.defs[f[1]]
                    stops = [num(st.get('stop-opacity'), 1.0) for st in g]
                    p0 = ap(mc, cx, cy)
                    self.ops.append(('glow', p0[0], p0[1], rx * mc[0], ry * mc[3], stops[0] * fo))
                else:
                    pts = [ap(mc, cx + rx * math.cos(a * math.pi / 16), cy + ry * math.sin(a * math.pi / 16)) for a in range(32)]
                    self.ops.append(('poly', pts, True, f and ('solid', f), fo, s, so, sw, dash, rc))
            elif t in ('polygon', 'polyline'):
                nums = [float(v) for v in re.split(r'[ ,]+', c.get('points').strip())]
                pts = [ap(mc, nums[k], nums[k + 1]) for k in range(0, len(nums), 2)]
                self.ops.append(('poly', pts, t == 'polygon', f and ('solid', f), fo, s, so, sw, dash, rc))
            elif t == 'path' and s and s[0] == 'url':
                g = self.defs[s[1]]
                stops = [(num(st.get('offset')), parse_color(st.get('stop-color')), num(st.get('stop-opacity'), 1.0)) for st in g]
                for pts, closed in flatten_path(c.get('d'), mc):
                    (x0, y0), (x1, y1) = pts[0], pts[-1]
                    n = 10
                    for k in range(n):
                        a0, a1 = k / n, (k + 1) / n
                        am = (a0 + a1) / 2
                        al = stops[0][2] + (stops[-1][2] - stops[0][2]) * am
                        seg = [(x0 + (x1 - x0) * a0, y0 + (y1 - y0) * a0), (x0 + (x1 - x0) * a1, y0 + (y1 - y0) * a1)]
                        self.ops.append(('poly', seg, False, None, 1.0, stops[0][1], al, sw, None, rc))
            elif t == 'path':
                for pts, closed in flatten_path(c.get('d'), mc):
                    if len(pts) >= 2:
                        fill = f and ('solid', f) if (closed or len(pts) >= 3) else None
                        self.ops.append(('poly', pts, closed, fill, fo, s, so, sw, dash, rc))
            elif t == 'text':
                x, y = num(c.get('x')), num(c.get('y'))
                p0 = ap(mc, x, y)
                self.ops.append(('text', p0[0], p0[1], num(c.get('font-size')) * ms, c.get('text-anchor', 'start'), f, fo, ''.join(c.itertext()), num(c.get('font-weight'), 400) >= 600))
            elif t == 'g':
                pass
            else:
                raise ValueError('unhandled ' + t)

    def collect_clips(self, e):
        for c in e:
            if self.tag(c) == 'clipPath':
                self.clips[c.get('id')] = c

    def clip_points(self, cp, m):
        out = []
        for c in cp:
            t = self.tag(c)
            if t == 'path':
                for pts, _ in flatten_path(c.get('d'), m):
                    out.extend(pts)
            elif t == 'rect':
                out.extend(rr_poly(num(c.get('x'), 0), num(c.get('y'), 0), num(c.get('width')), num(c.get('height')), 0, m))
            else:
                raise ValueError('clip ' + t)
        return out


def load(name, theme):
    tree = ET.parse(os.path.join(FIGS, '%s.%s.svg' % (name, theme)))
    w = Walker(tree.getroot())
    return w.run(), w.size


def color_fields(op):
    """Positions of (colour, alpha) tuples in an op, as (index) list."""
    k = op[0]
    if k == 'rect':
        return [6, 9]
    if k == 'circle':
        return [4, 7]
    if k == 'poly':
        return [None, 5]
    if k == 'text':
        return [5]
    return []


def ink_of(l, d, slots=None):
    if slots is not None:
        key = tuple(slots)
        if key not in SLOTS:
            SLOTS[key] = len(SLOTS)
        return 'Slot(%d)' % SLOTS[key]
    if l == '#000000':
        return 'Black'
    r = PAIRS.get((l, d))
    if r is None:
        raise KeyError((l, d))
    return r


SLOTS = collections.OrderedDict()


def f32(v):
    s = ('%.3f' % v).rstrip('0').rstrip('.')
    if s in ('', '-0'):
        s = '0'
    return s + ('' if '.' in s else '.0')


def paint(l, d, alpha, slots=None):
    """Option<Paint> source."""
    if l is None:
        return 'None'
    return 'Some(P { ink: Ink::%s, a: %s })' % (ink_of(l[0], d[0], slots), f32(alpha))


def line(sl, sd, so, sw, dash, rc, slots=None):
    if sl is None or sw <= 0:
        return 'None'
    ds = dash or (0, 0)
    return 'Some(L { ink: Ink::%s, a: %s, w: %s, dash: [%s, %s], round: %s })' % (ink_of(sl[0], sd[0], slots), f32(so), f32(sw), f32(ds[0]), f32(ds[1]), 'true' if rc else 'false')


def emit(name, ops_l, ops_d, slotsets=None):
    out = []
    assert len(ops_l) == len(ops_d), (name, len(ops_l), len(ops_d))
    for idx, (a, b) in enumerate(zip(ops_l, ops_d)):
        assert a[0] == b[0], (name, idx, a[0], b[0])
        sl = None
        if slotsets is not None:
            sl = [s[idx] for s in slotsets]
        k = a[0]
        if k == 'rect':
            _, x, y, w, h, r, f, fo, s, so, sw, dash = a
            _, _, _, _, _, _, fd, _, sd, _, _, _ = b
            fs = fsl = None
            if slotsets is not None:
                fs = [t[6][0] if t[6] else None for t in sl]
                ss = [t[8][0] if t[8] else None for t in sl]
            out.append('Op::Rect { r: [%s, %s, %s, %s], radius: %s, fill: %s, line: %s }' % (
                f32(x), f32(y), f32(w), f32(h), f32(r),
                paint(f, fd, fo, fs if slotsets is not None and f else None),
                line(s, sd, so, sw, dash, False, ss if slotsets is not None and s else None)))
        elif k == 'circle':
            _, x, y, r, f, fo, s, so, sw, dash = a
            fd, sd = b[4], b[6]
            fs = ss = None
            if slotsets is not None:
                fs = [t[4][0] if t[4] else None for t in sl]
                ss = [t[6][0] if t[6] else None for t in sl]
            out.append('Op::Circle { c: [%s, %s, %s], fill: %s, line: %s }' % (
                f32(x), f32(y), f32(r), paint(f, fd, fo, fs if f and slotsets is not None else None),
                line(s, sd, so, sw, dash, False, ss if s and slotsets is not None else None)))
        elif k == 'poly':
            _, pts, closed, fill, fo, s, so, sw, dash, rc = a
            fd, sd = b[3], b[5]
            pts_s = ', '.join('[%s, %s]' % (f32(px), f32(py)) for px, py in pts)
            fs = ss = None
            if slotsets is not None:
                fs = [t[3][1][0] if t[3] and t[3][0] == 'solid' else None for t in sl]
                ss = [t[5][0] if t[5] else None for t in sl]
            grad = 'None'
            fill_s = 'None'
            if fill and fill[0] == 'grad':
                _, top, bot, y0, y1 = fill
                gd = b[3]
                if slotsets is not None:
                    tops = [t[3][1] for t in sl]
                    bots = [t[3][2] for t in sl]
                    grad = 'Some(G { top: Ink::%s, bottom: Ink::%s, y: [%s, %s] })' % (ink_of(top, gd[1], tops), ink_of(bot, gd[2], bots), f32(y0), f32(y1))
                else:
                    grad = 'Some(G { top: Ink::%s, bottom: Ink::%s, y: [%s, %s] })' % (ink_of(top, gd[1]), ink_of(bot, gd[2]), f32(y0), f32(y1))
            elif fill:
                fill_s = paint(fill[1], fd[1], fo, fs if slotsets is not None else None)
            out.append('Op::Poly { pts: &[%s], closed: %s, fill: %s, grad: %s, line: %s }' % (
                pts_s, 'true' if closed else 'false', fill_s, grad,
                line(s, sd, so, sw, dash, rc, ss if s and slotsets is not None else None)))
        elif k == 'glow':
            _, x, y, rx, ry, al = a
            out.append('Op::Glow { c: [%s, %s, %s, %s], a: %s }' % (f32(x), f32(y), f32(rx), f32(ry), f32(al)))
        elif k == 'text':
            _, x, y, size, anchor, f, fo, txt, bold = a
            fd = b[5]
            fs = [t[5][0] for t in sl] if slotsets is not None else None
            out.append('Op::Text { at: [%s, %s], size: %s, anchor: %d, ink: Ink::%s, a: %s, s: %s, bold: %s }' % (
                f32(x), f32(y), f32(size), {'start': 0, 'middle': 1, 'end': 2}[anchor], ink_of(f[0], fd[0], fs), f32(fo), '"' + txt.replace('\\', '\\\\').replace('"', '\\"') + '"', 'true' if bold else 'false'))
        elif k == 'clip':
            pts_s = ', '.join('[%s, %s]' % (f32(px), f32(py)) for px, py in a[1])
            out.append('Op::Clip(&[%s])' % pts_s)
        elif k == 'unclip':
            out.append('Op::Unclip')
        else:
            raise ValueError(k)
    return out


def const_name(n):
    return n.upper().replace('-', '_')


def fn_name(n):
    return 'paint_' + n.replace('-', '_')


def main():
    man = sorted({f.split('.')[0] for f in os.listdir(FIGS) if f.endswith('.svg')})
    parts = []
    parts.append('//! Generated by `tools/preferences-figures/generate.py` from `docs/design/preferences/figures/*.svg`.\n'
                 '//! Do not edit by hand: change the SVG and run the script again.\n'
                 '#![allow(clippy::approx_constant, clippy::excessive_precision)]\n'
                 'use super::preferences_figure_ops::{Figure, G, Ink, L, Op, P};\n')
    fns = []
    names = []
    for name in man:
        if name in SKIP or name.startswith('theme-'):
            continue
        ol, size = load(name, 'light')
        od, size2 = load(name, 'dark')
        assert size == size2
        ops = emit(name, ol, od)
        parts.append('pub(super) const %s: Figure = Figure { size: [%s, %s], ops: &[\n    %s,\n] };\n' % (const_name(name), f32(size[0]), f32(size[1]), ',\n    '.join(ops)))
        names.append(name)
    # theme thumbnail: slots from the four themes
    loaded = [load(t, 'light')[0] for t in THEMES]
    size = load('theme-light', 'light')[1]
    ops = emit('theme-thumbnail', loaded[0], loaded[0], slotsets=loaded)
    parts.append('pub(super) const THEME_THUMBNAIL: Figure = Figure { size: [%s, %s], ops: &[\n    %s,\n] };\n' % (f32(size[0]), f32(size[1]), ',\n    '.join(ops)))
    slot_table = ['// Theme thumbnail slots (light, dark, gruvbox, imported sample colours):']
    for key, i in SLOTS.items():
        slot_table.append('//   %d: %s' % (i, ' '.join(str(k) for k in key)))
    parts.append('\n'.join(slot_table) + '\n')
    parts.append('use egui::{Painter, Rect};\nuse super::preferences_figure_ops::paint;\n')
    for n in names:
        parts.append('/// The `%s` figure of `docs/design/preferences/figures`.\npub(crate) fn %s(p: &Painter, rect: Rect) {\n    paint(p, rect, &%s, None, None);\n}\n' % (n, fn_name(n), const_name(n)))
    parts.append('/// Every figure painter by file name, for the gallery and the tests.\npub(crate) const FIGURES: &[(&str, [f32; 2], fn(&Painter, Rect))] = &[\n%s];\n' % ''.join('    ("%s", %s.size, %s),\n' % (n, const_name(n), fn_name(n)) for n in names))
    open(OUT, 'w', encoding='utf8', newline='\n').write('\n'.join(parts))
    print('figures', len(names), 'slots', len(SLOTS), file=sys.stderr)


if __name__ == '__main__':
    main()
