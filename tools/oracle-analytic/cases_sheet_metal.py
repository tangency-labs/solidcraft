#!/usr/bin/env python3
"""Closed-form expected values for the sheet-metal oracle cases (67..76).

Run from the worktree root:  python3 tools/oracle-analytic/cases_sheet_metal.py
It (re)writes tests/oracle/<case>/expected.json with "source": "analytic".

Folded sheet metal is built here from first principles as flat plates (prisms of thickness t)
and bends (annular-sector prisms: inner radius R, outer R + t, angle theta), joined face to
face.  The conventions that place them are SolidCraft's documented sheet-metal semantics
(crates/doc/src/sheet.rs module docs, docs/book/src/guide/sheet-metal.md):

  * Steel (mm) rule: t = 2.5, R = Thickness, K = 0.44, corner gap = Thickness.
  * Edge flange, height datum "outer faces", bend position "inside": the virtual sharp (where
    the outer faces would meet) lies on the picked edge's outer corner; the parent panel is
    trimmed back by the setback (R + t) tan(theta/2); the flange's outer face measures
    `height` from the virtual sharp (along the flange for a non-90 deg angle); the flat leg
    past the bend is height - (R + t) tan(theta/2).
  * Several flanges in one command meeting at a 90 deg corner: each bend stops at the trimmed
    base corner; each panel runs on past its bend by (R + t) - (t + gap/sqrt 2) so the gap
    between the panels' inner corners is the rule gap (corner open, no corner fill).
  * A later flange on an adjacent edge trims the base under the earlier bend; that bend stops
    at the trimmed base corner and the earlier panel runs on alone (an open corner relief).
  * Flat hem: 180 deg bend toward the face it starts from (inside), its leg hem_gap = 0.01 mm off
    that face (inner radius gap/2), length measured from the outer face of the hem bend,
    leg = L - (gap/2 + t).
  * Contour flange: the sketch lines are the sheet face on the side away from the material
    (material right of travel); a corner that bends toward the material has the sketch
    corner as the outer virtual sharp (setback (R+t)tan), the other as inner (setback R tan).
  * Unfold/refold: bend zone of flat width BA = theta (R + K t); a point s into the zone maps
    to angle s / (R + K t) on the bend; past the zone, flat distance maps 1:1 to the panel.
"""
import json
import math
import os

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
OUT = os.path.join(ROOT, "tests", "oracle")

PI = math.pi
STEEL = dict(t=2.5, R=2.5, K=0.44, gap=2.5, hem_gap=0.01)


# ---------------------------------------------------------------- 2D section pieces
class Poly:
    def __init__(self, pts):
        self.pts = pts

    def props(self):
        a = cx = cy = per = 0.0
        n = len(self.pts)
        for i in range(n):
            x0, y0 = self.pts[i]
            x1, y1 = self.pts[(i + 1) % n]
            c = x0 * y1 - x1 * y0
            a += c
            cx += (x0 + x1) * c
            cy += (y0 + y1) * c
            per += math.hypot(x1 - x0, y1 - y0)
        a *= 0.5
        return abs(a), (cx / (6 * a), cy / (6 * a)), per

    def box(self):
        xs = [p[0] for p in self.pts]
        ys = [p[1] for p in self.pts]
        return (min(xs), min(ys)), (max(xs), max(ys))


def Rect(u0, u1, v0, v1):
    return Poly([(u0, v0), (u1, v0), (u1, v1), (u0, v1)])


class Sector:
    """Annular sector: centre c, radii r0 < r1, angles a0 < a1 (radians, CCW)."""

    def __init__(self, c, r0, r1, a0, a1):
        self.c, self.r0, self.r1, self.a0, self.a1 = c, r0, r1, a0, a1

    def props(self):
        d = self.a1 - self.a0
        r0, r1 = self.r0, self.r1
        a = d / 2 * (r1 * r1 - r0 * r0)
        rho = (2.0 / 3.0) * (r1 ** 3 - r0 ** 3) / (r1 * r1 - r0 * r0) * math.sin(d / 2) / (d / 2)
        m = (self.a0 + self.a1) / 2
        cen = (self.c[0] + rho * math.cos(m), self.c[1] + rho * math.sin(m))
        per = d * (r0 + r1) + 2 * (r1 - r0)
        return a, cen, per

    def box(self):
        pts = []
        for r in (self.r0, self.r1):
            for ang in (self.a0, self.a1):
                pts.append((r * math.cos(ang), r * math.sin(ang)))
        k = math.ceil(self.a0 / (PI / 2) - 1e-12)
        while k * PI / 2 <= self.a1 + 1e-12:
            ang = k * PI / 2
            pts.append((self.r1 * math.cos(ang), self.r1 * math.sin(ang)))
            k += 1
        xs = [p[0] + self.c[0] for p in pts]
        ys = [p[1] + self.c[1] for p in pts]
        return (min(xs), min(ys)), (max(xs), max(ys))


class Section:
    """Union of pieces meeting face to face; `joints` = total length of shared interfaces."""

    def __init__(self, pieces, joints):
        self.pieces, self.joints = pieces, joints

    def props(self):
        A = Su = Sv = P = 0.0
        for p in self.pieces:
            a, (cu, cv), per = p.props()
            A += a
            Su += a * cu
            Sv += a * cv
            P += per
        return A, (Su / A, Sv / A), P - 2 * self.joints

    def box(self):
        bs = [p.box() for p in self.pieces]
        return (min(b[0][0] for b in bs), min(b[0][1] for b in bs)), (max(b[1][0] for b in bs), max(b[1][1] for b in bs))


def extrusion(segments, to_world):
    """A solid swept along one axis w: segments = [(w0, w1, Section)], contiguous, with each
    section nested in its neighbours (so a step's cap area is the area difference).
    to_world(u, v, w) -> (x, y, z).  Returns (V, A, com, bbox)."""
    V = A = 0.0
    S = [0.0, 0.0, 0.0]
    lo = [1e300] * 3
    hi = [-1e300] * 3
    areas = []
    for w0, w1, sec in segments:
        a, (cu, cv), per = sec.props()
        L = w1 - w0
        V += a * L
        A += per * L
        c = to_world(cu, cv, (w0 + w1) / 2)
        for i in range(3):
            S[i] += a * L * c[i]
        (u0, v0), (u1, v1) = sec.box()
        for u in (u0, u1):
            for v in (v0, v1):
                for w in (w0, w1):
                    p = to_world(u, v, w)
                    for i in range(3):
                        lo[i] = min(lo[i], p[i])
                        hi[i] = max(hi[i], p[i])
        areas.append(a)
    A += areas[0] + areas[-1]
    for i in range(len(areas) - 1):
        A += abs(areas[i] - areas[i + 1])
    return V, A, [s / V for s in S], (lo, hi)


# ---------------------------------------------------------------- 3D prisms (corner cases)
def prism(sec_piece, w0, w1, to_world):
    a, (cu, cv), per = sec_piece.props()
    L = w1 - w0
    c = to_world(cu, cv, (w0 + w1) / 2)
    (u0, v0), (u1, v1) = sec_piece.box()
    pts = [to_world(u, v, w) for u in (u0, u1) for v in (v0, v1) for w in (w0, w1)]
    lo = [min(p[i] for p in pts) for i in range(3)]
    hi = [max(p[i] for p in pts) for i in range(3)]
    return dict(V=a * L, A=2 * a + per * L, c=c, lo=lo, hi=hi)


def assemble(prisms, contacts, extra_V=0.0, extra_A=0.0, extra_S=(0.0, 0.0, 0.0)):
    V = sum(p["V"] for p in prisms) + extra_V
    A = sum(p["A"] for p in prisms) - 2 * sum(contacts) + extra_A
    com = [(sum(p["V"] * p["c"][i] for p in prisms) + extra_S[i]) / V for i in range(3)]
    lo = [min(p["lo"][i] for p in prisms) for i in range(3)]
    hi = [max(p["hi"][i] for p in prisms) for i in range(3)]
    return V, A, com, (lo, hi)


# ---------------------------------------------------------------- shared shapes
def yz(u, v, w):  # section in (y, z), swept along x
    return (w, u, v)


def xz(u, v, w):  # section in (x, z), swept along y
    return (u, w, v)


def edge_flange_section(width_y, height, theta, rv, base_far=None):
    """Base plate y in [0, width_y] with an edge flange on its y = 0 top edge (bend up, toward
    -y), as YZ section pieces.  Returns (pieces, joints, info)."""
    t, R = rv["t"], rv["R"]
    sb = (R + t) * math.tan(theta / 2)
    leg = height - sb
    zc = t + R
    base = Rect(sb, width_y, 0.0, t)
    bend = Sector((sb, zc), R, R + t, -PI / 2 - theta, -PI / 2)
    # Panel: flat (s along -y from the bend line, z in [0, t]) turned by theta about the axis.
    c, s = math.cos(theta), math.sin(theta)

    def place(sv, z):
        uu, ww = sv, z - zc  # uu outward (-y) from the axis, ww from the axis
        ru, rw = uu * c - ww * s, uu * s + ww * c
        return (sb - ru, zc + rw)

    panel = Poly([place(0, 0), place(0, t), place(leg, t), place(leg, 0)])
    return [base, bend, panel], 2 * t, dict(setback=sb, leg=leg)


# Cases SolidCraft fails today, with the issue that tracks each (cargo xtask oracle reports them
# as known failures and fails once they pass, so the mark is removed with the fix).
KNOWN_FAILURES = {"75-sm-bracket-holes": "#296"}


def write(case, d):
    if case in KNOWN_FAILURES:
        d["known_failure"] = KNOWN_FAILURES[case]
    path = os.path.join(OUT, case, "expected.json")
    with open(path, "w") as f:
        json.dump(d, f, indent=2)
        f.write("\n")


def clean(v):
    """Drop float noise (e.g. 1e-15 from cos(pi/2)) on bbox coordinates."""
    r = round(v, 9)
    return r if abs(v - r) < 1e-12 else v


def record(V, A, com, bb, method, faces=None, note=None, body_count=1):
    bb = ([clean(v) + 0.0 for v in bb[0]], [clean(v) + 0.0 for v in bb[1]])
    d = {
        "source": "analytic",
        "method": method,
        "body_count": body_count,
        "volume_mm3": V,
        "area_mm2": A,
        "center_of_mass": com,
        "bbox": {"min": bb[0], "max": bb[1]},
    }
    if faces is not None:
        d["faces"] = faces
    if note:
        d["note"] = note
    return d


# ---------------------------------------------------------------- cases
def c67():
    t = STEEL["t"]
    sec = Section([Rect(0, 60, 0, t)], 0)
    V, A, com, bb = extrusion([(0, 100, sec)], yz)
    return record(V, A, com, bb, "plate 100 x 60 x t, t = 2.5 (Steel (mm) rule), thickness along +Z: V = 100*60*t, A = 2(ab+bc+ca)", faces=6)


def single_flange(width_x, width_y, height, theta, rv, method_extra):
    pieces, joints, info = edge_flange_section(width_y, height, theta, rv)
    V, A, com, bb = extrusion([(0, width_x, Section(pieces, joints))], yz)
    return V, A, com, bb, info


def c68():
    rv = STEEL
    V, A, com, bb, info = single_flange(100, 60, 20, PI / 2, rv, "")
    m = ("extrusion along x (100) of the YZ section: base y 5..60 x t; 90 deg bend annulus R 2.5..5 about (y 5, z 5); "
         "flange y 0..2.5, z 5..20. Setback (R+t)tan45 = 5 (inside bend position, outer-face height datum: "
         "outer face of flange reaches z = 20); V = L*A_sec, A = 2 A_sec + L P_sec")
    return record(V, A, com, bb, m, faces=None,
                  note="trim/setback and height datum are SolidCraft's documented edge-flange conventions (sheet.rs add_flanges)")


def c70():
    rv = STEEL
    th = PI / 4
    V, A, com, bb, info = single_flange(100, 60, 15, th, rv, "")
    m = ("as 68 with a 45 deg bend: setback (R+t)tan22.5 = %.6f, base y %.6f..60, bend annulus 2.5..5 over 45 deg about "
         "(y %.6f, z 5), flat leg 15 - setback = %.6f turned 45 deg; extrusion along x 100" % (info["setback"], info["setback"], info["setback"], info["leg"]))
    return record(V, A, com, bb, m, faces=None,
                  note="SolidCraft convention: flange height (15) is measured along the flange's outer face from the virtual sharp at the original edge (y 0, z 0), so the outer top corner is at (y, z) = (-15 cos45, 15 sin45).")


def u_channel(rv, wx, wy, h):
    t, R = rv["t"], rv["R"]
    sb = R + t
    zc = t + R
    leg = h - sb
    pieces = [
        Rect(sb, wy - sb, 0, t),
        Sector((sb, zc), R, R + t, -PI, -PI / 2),
        Rect(0, t, zc, zc + leg),
        Sector((wy - sb, zc), R, R + t, -PI / 2, 0),
        Rect(wy - t, wy, zc, zc + leg),
    ]
    return pieces, sb, leg


def c71():
    rv = dict(t=1.5, R=3.0, K=0.33, gap=1.5)
    pieces, sb, leg = u_channel(rv, 80, 50, 20)
    V, A, com, bb = extrusion([(0, 80, Section(pieces, 4 * rv["t"]))], yz)
    m = ("U channel, rule t 1.5, R 3, K 0.33 (K unused folded): base y 4.5..45.5 x 1.5; two 90 deg bend annuli R 3..4.5; "
         "flanges y 0..1.5 and 48.5..50, z 4.5..20; extrusion along x 80")
    return record(V, A, com, bb, m, faces=None,
                  note="the rule edit applies to the whole sheet body (base built before it is 1.5 thick too, as the recipe's edge picks at z = 1.5 state)")


def c72():
    rv = STEEL
    t, R = rv["t"], rv["R"]
    # Sketch lines (world XZ): (0,0)->(40,0)->(40,30)->(70,30), material right of travel (+z, then -x, then +z).
    pieces = [
        Rect(0, 40 - (R + t), 0, t),                      # x 0..35
        Sector((40 - (R + t), R + t), R, R + t, -PI / 2, 0),  # centre (35, 5)
        Rect(40 - t, 40, R + t, 30 - R),                  # x 37.5..40, z 5..27.5
        Sector((40 + R, 30 - R), R, R + t, PI / 2, PI),   # centre (42.5, 27.5)
        Rect(40 + R, 70, 30, 30 + t),                     # x 42.5..70, z 30..32.5
    ]
    V, A, com, bb = extrusion([(0, 50, Section(pieces, 4 * t))], xz)
    m = ("contour flange, width 50 along +Y, XZ section: plate x 0..35 z 0..2.5; bend R 2.5..5 about (35, 5) (sketch corner = "
         "outer virtual sharp, setback R+t); wall x 37.5..40 z 5..27.5; bend R 2.5..5 about (42.5, 27.5) (sketch corner = inner "
         "virtual sharp, setback R); plate x 42.5..70 z 30..32.5")
    return record(V, A, com, bb, m, faces=None,
                  note="SolidCraft convention: the sketch lines are the sheet face away from the material (material right of travel); thickness grows to the material side")


def c73():
    rv = STEEL
    t, R, g = rv["t"], rv["R"], rv["hem_gap"]
    pieces, joints, info = edge_flange_section(60, 20, PI / 2, rv)
    L = 8.0
    r = g / 2
    leg = L - (r + t)
    hem_bend = Sector((t + r, 20.0), r, r + t, 0.0, PI)
    hem_panel = Rect(t + g, 2 * t + g, 20.0 - leg, 20.0)
    sec = Section(pieces + [hem_bend, hem_panel], joints + 2 * t)
    V, A, com, bb = extrusion([(0, 100, sec)], yz)
    m = ("68's part (height 20) plus a flat hem on the flange top edge: 180 deg bend, inner radius = hem gap 0.01 / 2, outer 2.505, "
         "about (y 2.505, z 20), folded toward +y (inside); hem leg 8 - (0.005 + 2.5) = 5.495: y 2.51..5.01, z 14.505..20; "
         "extrusion along x 100")
    return record(V, A, com, bb, m, faces=None,
                  note="SolidCraft conventions: hem gap 0.01 mm (rule hem_gap) between the hem's leg and the face it folds over, hem length measured to the outer face of the hem bend (top at z 22.505), hem folds over the flange's inner face.")


def c74():
    rv = STEEL
    t, R, K = rv["t"], rv["R"], rv["K"]
    sb = R + t
    zc = t + R
    leg = 20 - sb
    BA = PI / 2 * (R + K * t)
    # Flat: base y 5..45; y=0 side bend zone y (5-BA)..5, panel y (5-BA-15)..(5-BA).
    # Slot y -12..10: base strip 5..10, whole bend, panel up to 12 - (BA - 5) from the bend.
    cut_panel = -(-12 - (sb - BA))  # flat distance past the bend zone
    zcut = zc + cut_panel
    full = Section([
        Rect(sb, 50 - sb, 0, t),
        Sector((sb, zc), R, R + t, -PI, -PI / 2),
        Rect(0, t, zc, zc + leg),
        Sector((50 - sb, zc), R, R + t, -PI / 2, 0),
        Rect(50 - t, 50, zc, zc + leg),
    ], 4 * t)
    cut = Section([
        Rect(10, 50 - sb, 0, t),
        Sector((50 - sb, zc), R, R + t, -PI / 2, 0),
        Rect(50 - t, 50, zc, zc + leg),
        Rect(0, t, zcut, zc + leg),
    ], 2 * t)
    V, A, com, bb = extrusion([(0, 35, full), (35, 45, cut), (45, 80, full)], yz)
    m = ("U channel 80x50 (t 2.5, R 2.5, base y 5..45, flanges z 5..20) with the flat-pattern slot x 35..45, y -12..10 refolded: "
         "BA = (pi/2)(R + K t) = %.9f, so the slot removes base y 5..10, the whole y=0 bend and the flange from z 5 to "
         "5 + (12 - (BA - 5)) = %.9f; x-segments 0..35 / 35..45 / 45..80" % (BA, zcut))
    return record(V, A, com, bb, m, faces=None,
                  note="unfold/refold maps flat distance through the K-factor neutral layer (K 0.44)")


def c75():
    rv = STEEL
    t, R = rv["t"], rv["R"]
    sb = R + t
    zc = t + R
    leg1, leg2 = 30 - sb, 15 - sb
    P = []
    P.append(prism(Rect(sb, 40, 0, t), 0, 80 - sb, yz))                  # base x 0..75, y 5..40
    P.append(prism(Sector((sb, zc), R, R + t, -PI, -PI / 2), 0, 80 - sb, yz))  # bend 1, x 0..75
    P.append(prism(Rect(0, t, zc, zc + leg1), 0, 80, yz))                # flange 1, x 0..80
    P.append(prism(Sector((80 - sb, zc), R, R + t, -PI / 2, 0), sb, 40, xz))  # bend 2, y 5..40
    P.append(prism(Rect(80 - t, 80, zc, zc + leg2), sb, 40, xz))         # flange 2, y 5..40
    contacts = [(80 - sb) * t, (80 - sb) * t, (40 - sb) * t, (40 - sb) * t]
    r, n = 3.0, 2
    dV = -n * PI * r * r * t
    dA = n * (2 * PI * r * t - 2 * PI * r * r)
    dS = (-PI * r * r * t * (25 + 55), -n * PI * r * r * t * 20, -n * PI * r * r * t * t / 2)
    V, A, com, bb = assemble(P, contacts, dV, dA, dS)
    m = ("base x 0..75, y 5..40, z 0..2.5 (trimmed 5 by each flange's setback); bend 1 (R 2.5..5 about y 5, z 5) x 0..75; "
         "flange 1 y 0..2.5, z 5..30, x 0..80; bend 2 (R 2.5..5 about x 75, z 5) y 5..40; flange 2 x 77.5..80, z 5..15, "
         "y 5..40; minus two d6 through holes at (25,20), (55,20) in the base. A = sum of prism areas - 2 x 4 joint "
         "areas (75t, 75t, 35t, 35t) + hole walls - hole discs")
    return record(V, A, com, bb, m, faces=None,
                  note="SolidCraft convention: the second flange trims the base under the first bend, so bend 1 stops at x 75 while flange 1's panel runs to x 80 (open corner relief, no fill).")


def c69():
    rv = STEEL
    t, R, gap = rv["t"], rv["R"], rv["gap"]
    sb = R + t
    zc = t + R
    leg = 25 - sb
    ext = sb - (t + gap / math.sqrt(2))
    X, Y = 100.0, 60.0
    P = [prism(Rect(sb, Y - sb, 0, t), sb, X - sb, yz)]  # base x 5..95, y 5..55
    # y = 0 and y = 60 flanges (swept along x)
    P.append(prism(Sector((sb, zc), R, R + t, -PI, -PI / 2), sb, X - sb, yz))
    P.append(prism(Rect(0, t, zc, zc + leg), sb - ext, X - sb + ext, yz))
    P.append(prism(Sector((Y - sb, zc), R, R + t, -PI / 2, 0), sb, X - sb, yz))
    P.append(prism(Rect(Y - t, Y, zc, zc + leg), sb - ext, X - sb + ext, yz))
    # x = 0 and x = 100 flanges (swept along y)
    P.append(prism(Sector((sb, zc), R, R + t, -PI, -PI / 2), sb, Y - sb, xz))
    P.append(prism(Rect(0, t, zc, zc + leg), sb - ext, Y - sb + ext, xz))
    P.append(prism(Sector((X - sb, zc), R, R + t, -PI / 2, 0), sb, Y - sb, xz))
    P.append(prism(Rect(X - t, X, zc, zc + leg), sb - ext, Y - sb + ext, xz))
    lx, ly = X - 2 * sb, Y - 2 * sb
    contacts = [lx * t] * 4 + [ly * t] * 4
    V, A, com, bb = assemble(P, contacts)
    m = ("tray: base 90 x 50 x 2.5 (each edge trimmed by the setback 5); four 90 deg bends R 2.5..5 along the trimmed edges "
         "(90, 50, 90, 50 long); four walls 2.5 thick, z 5..25, each running on past its bend by "
         "ext = (R+t) - (t + gap/sqrt2) = %.9f at both ends; open corners. A = sum of prisms - 2 x joints" % ext)
    return record(V, A, com, bb, m, faces=46,
                  note="corner treatment is SolidCraft's convention (sheet.rs close_corners: panels extended until their inner corners are the rule gap = t apart; no corner fill, open corner relief below the walls)")


def c76():
    rv = dict(t=3.0, R=3.0, K=0.44, gap=3.0)
    V, A, com, bb, info = single_flange(100, 60, 20, PI / 2, rv, "")
    m = ("final rule thickness 3 (bend radius = Thickness = 3): base y 6..60 x 3; bend annulus R 3..6 about (y 6, z 6); "
         "flange y 0..3, z 6..20; extrusion along x 100")
    return record(V, A, com, bb, m, faces=None,
                  note="the rule's thickness edits regenerate the whole sheet body; only the final 3 mm state matters")


CASES = {
    "67-sm-base-flange": c67,
    "68-sm-edge-flange": c68,
    "69-sm-tray": c69,
    "70-sm-flange-45": c70,
    "71-sm-custom-rule": c71,
    "72-sm-contour-flange": c72,
    "73-sm-hem": c73,
    "74-sm-unfold-refold": c74,
    "75-sm-bracket-holes": c75,
    "76-sm-thickness-variant": c76,
}

if __name__ == "__main__":
    for case, fn in CASES.items():
        d = fn()
        write(case, d)
        print(case, d["volume_mm3"], d["area_mm2"], [round(c, 6) for c in d["center_of_mass"]])
