#!/usr/bin/env python3
"""Closed-form expected values for the basic oracle cases.

Run from the worktree root:  python3 tools/oracle-analytic/cases_basic.py [case ...]
Writes tests/oracle/<case>/expected.json with "source": "analytic".

Every value is derived from first principles (prisms, cylinders, cones/frustums, spheres and
caps, tori via Pappus, Green's theorem over line/arc profiles) and composed by adding or
subtracting mass properties. Nothing here is measured from a CAD kernel.
Stdlib only.
"""
import json
import math
import os
import sys

PI = math.pi
ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "tests", "oracle")


# ----------------------------------------------------------------------------------------------
# Mass-property composition
# ----------------------------------------------------------------------------------------------
class MP:
    """Volume and first moments; add pieces with sign +1 (material) or -1 (removed)."""

    def __init__(self):
        self.v = 0.0
        self.m = [0.0, 0.0, 0.0]

    def add(self, v, c, sign=1.0):
        self.v += sign * v
        for i in range(3):
            self.m[i] += sign * v * c[i]
        return self

    def com(self):
        return [self.m[i] / self.v for i in range(3)]


def box_mp(lo, hi):
    v = (hi[0] - lo[0]) * (hi[1] - lo[1]) * (hi[2] - lo[2])
    return v, [(lo[i] + hi[i]) / 2 for i in range(3)]


def box_area(a, b, c):
    return 2 * (a * b + b * c + c * a)


def frustum_v(h, r1, r2):
    """Circular frustum, radii r1 (z=0) and r2 (z=h)."""
    return PI * h / 3 * (r1 * r1 + r1 * r2 + r2 * r2)


def frustum_zc(h, r1, r2):
    """Centroid height of a circular frustum above the r1 end."""
    return h * (r1 * r1 + 2 * r1 * r2 + 3 * r2 * r2) / (4 * (r1 * r1 + r1 * r2 + r2 * r2))


def frustum_lat(h, r1, r2):
    return PI * (r1 + r2) * math.hypot(h, r1 - r2)


def sq_frustum(h, a0, a1):
    """Square frustum, side a0 at z=0, a1 at z=h: (V, z-centroid)."""
    A0, A1 = a0 * a0, a1 * a1
    s = math.sqrt(A0 * A1)
    v = h / 3 * (A0 + A1 + s)
    zc = h * (A0 + 2 * s + 3 * A1) / (4 * (A0 + s + A1))
    return v, zc


def cap_v(R, h):
    return PI * h * h * (3 * R - h) / 3


# ----------------------------------------------------------------------------------------------
# 2D region integrals by Green's theorem (lines exact; arcs by 48-point Gauss-Legendre, which
# integrates these low-degree trigonometric polynomials to machine precision).
# ----------------------------------------------------------------------------------------------
def _gauss_legendre(n):
    xs, ws = [], []
    for i in range(1, n + 1):
        x = math.cos(PI * (i - 0.25) / (n + 0.5))
        for _ in range(100):
            p0, p1 = 1.0, x
            for k in range(2, n + 1):
                p0, p1 = p1, ((2 * k - 1) * x * p1 - (k - 1) * p0) / k
            dp = n * (x * p1 - p0) / (x * x - 1)
            dx = p1 / dp
            x -= dx
            if abs(dx) < 1e-16:
                break
        xs.append(x)
        ws.append(2 / ((1 - x * x) * dp * dp))
    return xs, ws


GL = _gauss_legendre(48)


def seg_eval(seg, t):
    """Point and derivative on a segment at t in [0, 1]."""
    if seg[0] == "line":
        (x0, y0), (x1, y1) = seg[1], seg[2]
        return x0 + (x1 - x0) * t, y0 + (y1 - y0) * t, x1 - x0, y1 - y0
    _, (cx, cy), r, a0, a1 = seg
    a = a0 + (a1 - a0) * t
    da = a1 - a0
    return cx + r * math.cos(a), cy + r * math.sin(a), -r * math.sin(a) * da, r * math.cos(a) * da


def seg_int(seg, f):
    """Integral over t of f(x, y, dx/dt, dy/dt)."""
    s = 0.0
    for x, w in zip(*GL):
        t = (x + 1) / 2
        s += w / 2 * f(*seg_eval(seg, t))
    return s


def region(segs):
    """Area A, moments Sx = int x dA, Sy = int y dA, Sxy = int x*y dA of a CCW-bounded region."""
    A = sum(seg_int(s, lambda x, y, dx, dy: x * dy) for s in segs)
    Sx = sum(seg_int(s, lambda x, y, dx, dy: x * x / 2 * dy) for s in segs)
    Sy = sum(seg_int(s, lambda x, y, dx, dy: x * y * dy) for s in segs)
    Sxy = sum(seg_int(s, lambda x, y, dx, dy: x * x * y / 2 * dy) for s in segs)
    return A, Sx, Sy, Sxy


def seg_len(seg):
    return seg_int(seg, lambda x, y, dx, dy: math.hypot(dx, dy))


def seg_rev_area(seg):
    """Area swept by a profile segment (x = radius) turned 360 deg about the y axis."""
    return seg_int(seg, lambda x, y, dx, dy: 2 * PI * x * math.hypot(dx, dy))


def arc3(p0, pm, p1):
    """Circle through three points, as an arc segment from p0 through pm to p1."""
    (ax, ay), (bx, by), (cx, cy) = p0, pm, p1
    d = 2 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by))
    ux = ((ax * ax + ay * ay) * (by - cy) + (bx * bx + by * by) * (cy - ay) + (cx * cx + cy * cy) * (ay - by)) / d
    uy = ((ax * ax + ay * ay) * (cx - bx) + (bx * bx + by * by) * (ax - cx) + (cx * cx + cy * cy) * (bx - ax)) / d
    r = math.hypot(ax - ux, ay - uy)
    t0 = math.atan2(ay - uy, ax - ux)
    tm = math.atan2(by - uy, bx - ux)
    t1 = math.atan2(cy - uy, cx - ux)
    # choose the sweep direction that passes through the mid point
    ccw = (t1 - t0) % (2 * PI)
    if (tm - t0) % (2 * PI) < ccw:
        a1 = t0 + ccw
    else:
        a1 = t0 - ((t0 - t1) % (2 * PI))
    return ("arc", (ux, uy), r, t0, a1)


def circle_seg(c, r):
    return ("arc", c, r, 0.0, 2 * PI)


# ----------------------------------------------------------------------------------------------
# Output
# ----------------------------------------------------------------------------------------------
def result(method, bodies, V, A, com, lo, hi, faces=None, note=None, **extra):
    d = {
        "source": "analytic",
        "method": method,
        "body_count": bodies,
        "volume_mm3": V,
        "area_mm2": A,
        "center_of_mass": [c + 0.0 for c in com],
        "bbox": {"min": [float(x) + 0.0 for x in lo], "max": [float(x) + 0.0 for x in hi]},
    }
    if faces is not None:
        d["faces"] = faces
    if note:
        d["note"] = note
    d.update(extra)
    return d


# ----------------------------------------------------------------------------------------------
# Cases
# ----------------------------------------------------------------------------------------------
def c01_box():
    a, b, c = 40.0, 30.0, 20.0
    return result("box 40 x 30 x 20 from the origin: V = abc, A = 2(ab + bc + ca)", 1, a * b * c, box_area(a, b, c),
                  [a / 2, b / 2, c / 2], [0, 0, 0], [a, b, c], 6)


def c02_cylinder():
    r, h = 15.0, 50.0
    return result("cylinder r=15 h=50 on XY: V = pi r^2 h, A = 2 pi r^2 + 2 pi r h", 1, PI * r * r * h,
                  2 * PI * r * r + 2 * PI * r * h, [0, 0, h / 2], [-r, -r, 0], [r, r, h], 3)


def c03_box_through_hole():
    a, b, c, r = 60.0, 40.0, 20.0, 6.0
    V = a * b * c - PI * r * r * c
    A = box_area(a, b, c) - 2 * PI * r * r + 2 * PI * r * c
    return result("box 60x40x20 minus centred through cylinder r=6: V = abc - pi r^2 c, A = 2(ab+bc+ca) - 2 pi r^2 + 2 pi r c; "
                  "hole is centred so the centroid stays at the box centre", 1, V, A, [30, 20, 10], [0, 0, 0], [a, b, c], 7)


def _vase_profile():
    # sketch on XZ with y_dir = -Z: world (x, z) = (sx, -sy)
    w = lambda p: (p[0], -p[1])
    P = [w(p) for p in [(0, 0), (20, 0), (25, -30)]]
    arc = arc3(w((25, -30)), w((23, -46)), w((15, -60)))
    Q = [w(p) for p in [(15, -60), (18, -80), (0, -80)]]
    segs = [("line", P[0], P[1]), ("line", P[1], P[2]), arc, ("line", Q[0], Q[1]), ("line", Q[1], Q[2]), ("line", Q[2], P[0])]
    return segs, arc


def c04_revolve_vase():
    segs, arc = _vase_profile()
    A2, Sx, Sy, Sxy = region(segs)
    V = 2 * PI * Sx
    zc = Sxy / Sx
    area = sum(seg_rev_area(s) for s in segs)
    # largest radius: the line/arc endpoints, or the arc's x-extreme if it lies within the sweep
    _, (cx, cz), r, a0, a1 = arc
    rmax = max(25.0, 20.0, 15.0, 18.0)
    lo_a, hi_a = min(a0, a1), max(a0, a1)
    for k in range(-2, 3):
        if lo_a <= 2 * PI * k <= hi_a:
            rmax = max(rmax, cx + r)
    method = ("revolve of a line/3-point-arc profile (XZ, x = radius, z = -sketch y) 360 deg about Z. Pappus/Green: "
              "V = 2 pi * int x dA, z_c = int x z dA / int x dA, A = sum over profile edges of 2 pi int x ds "
              "(bottom disk r20, frustum r20->25, torus zone of the arc (centre (%.6f, %.6f), R=%.6f), frustum r15->18, top disk r18)"
              % (cx, cz, r))
    return result(method, 1, V, area, [0, 0, zc], [-rmax, -rmax, 0], [rmax, rmax, 80], 5)


def c05_taper_extrude():
    h, a0, t = 30.0, 40.0, math.radians(10)
    a1 = a0 + 2 * h * math.tan(t)  # positive taper offsets the CCW outer loop outward (recipe.rs -> extrude_tapered)
    V, zc = sq_frustum(h, a0, a1)
    side = (a0 + a1) / 2 * h / math.cos(t)
    A = a0 * a0 + a1 * a1 + 4 * side
    return result("square frustum: base 40 at z=0, top a1 = 40 + 2*30*tan10 at z=30 (positive taper grows outward); "
                  "V = h/3 (A0 + A1 + sqrt(A0 A1)), z_c = h (A0 + 2 sqrt(A0A1) + 3 A1) / (4 (A0 + sqrt(A0A1) + A1)), "
                  "sides = 4 trapezoids (a0+a1)/2 * h/cos(10)", 1, V, A, [0, 0, zc], [-a1 / 2, -a1 / 2, 0], [a1 / 2, a1 / 2, h], 6)


def c06_symmetric_extrude():
    a, b, c = 50.0, 30.0, 20.0
    return result("50 (x) by 20 (z) rectangle on XZ extruded +-15 along Y: box 50x30x20 centred at origin", 1, a * b * c,
                  box_area(a, b, c), [0, 0, 0], [-25, -15, -10], [25, 15, 10], 6)


def c07_fillet_all_edges():
    r = 3.0
    a, b, c = 40 - 2 * r, 30 - 2 * r, 20 - 2 * r  # inner box swept by a ball of radius r
    V = a * b * c + 2 * r * (a * b + b * c + c * a) + PI * r * r * (a + b + c) + 4 / 3 * PI * r ** 3
    A = 2 * (a * b + b * c + c * a) + 2 * PI * r * (a + b + c) + 4 * PI * r * r
    return result("box 40x30x20 with every edge rounded r=3 = inner box (34x24x14) grown by a ball r (Steiner): "
                  "V = abc + 2r(ab+bc+ca) + pi r^2 (a+b+c) + 4/3 pi r^3, A = 2(ab+bc+ca) + 2 pi r (a+b+c) + 4 pi r^2; "
                  "faces 6 planes + 12 quarter cylinders + 8 sphere octants", 1, V, A, [20, 15, 10], [0, 0, 0], [40, 30, 20], 26)


def c08_chamfer():
    d = 2.0
    mp = MP().add(*box_mp([0, 0, 0], [40, 30, 18]))
    # top slab z in [18, 20]: at depth u below the top the section is (36+2u) x (26+2u)
    # V = int_0^2 (36+2u)(26+2u) du ; z-moment = int (20-u)(...) du (polynomial, exact)
    f = lambda u: (36 + 2 * u) * (26 + 2 * u)
    # integrate quadratic exactly (Simpson)
    Vs = d / 6 * (f(0) + 4 * f(1) + f(2))
    g = lambda u: (20 - u) * f(u)  # cubic: Simpson is exact
    Ms = d / 6 * (g(0) + 4 * g(1) + g(2))
    mp.add(Vs, [20, 15, Ms / Vs])
    s = d * math.sqrt(2)
    A = 40 * 30 + 2 * (40 + 30) * 18 + 36 * 26 + 2 * (40 + 36) / 2 * s + 2 * (30 + 26) / 2 * s
    return result("box 40x30x20 with the 4 top edges chamfered 2: lower box 40x30x18 + top slab with section (36+2u)x(26+2u) at depth u "
                  "(V, moment by exact polynomial integration); A = bottom + sides(h=18) + top 36x26 + 4 trapezoid bevels of slant 2 sqrt2",
                  1, mp.v, A, mp.com(), [0, 0, 0], [40, 30, 20], 10)


def c09_shell():
    t = 2.0
    mp = MP().add(*box_mp([0, 0, 0], [50, 40, 30])).add(*box_mp([t, t, t], [50 - t, 40 - t, 30]), -1)
    ia, ib, ih = 50 - 2 * t, 40 - 2 * t, 30 - t
    A = 50 * 40 + 2 * (50 + 40) * 30 + (50 * 40 - ia * ib) + ia * ib + 2 * (ia + ib) * ih
    return result("50x40x30 box shelled inward 2 with the top open: box minus cavity [2,48]x[2,38]x[2,30]; "
                  "A = outer bottom + outer sides + rim + cavity floor + cavity walls", 1, mp.v, A, mp.com(), [0, 0, 0], [50, 40, 30], 11)


def c10_rect_pattern_holes():
    r, t = 3.0, 5.0
    n = 12
    mp = MP().add(*box_mp([0, 0, 0], [90, 70, t]))
    for i in range(4):
        for j in range(3):
            mp.add(PI * r * r * t, [15 + 20 * i, 15 + 20 * j, t / 2], -1)
    A = box_area(90, 70, t) + n * (2 * PI * r * t - 2 * PI * r * r)
    return result("plate 90x70x5 minus 12 through cylinders r=3 at (15+20i, 15+20j), i<4, j<3: V = abc - 12 pi r^2 t, "
                  "A = 2(ab+bc+ca) + 12(2 pi r t - 2 pi r^2)", 1, mp.v, A, mp.com(), [0, 0, 0], [90, 70, t], 18)


def c11_circular_pattern_flange():
    R, ri, rh, h = 50.0, 15.0, 5.0, 10.0
    V = PI * (R * R - ri * ri) * h - 6 * PI * rh * rh * h
    A = 2 * PI * (R * R - ri * ri) + 2 * PI * (R + ri) * h + 6 * (2 * PI * rh * h - 2 * PI * rh * rh)
    return result("annulus R50/r15 h10 minus 6 through cylinders r=5 on a 40 radius: V = pi(R^2-r^2)h - 6 pi rh^2 h, "
                  "A = 2 pi(R^2-r^2) + 2 pi(R+r)h + 6(2 pi rh h - 2 pi rh^2); 6-fold symmetry keeps the centroid on the axis",
                  1, V, A, [0, 0, h / 2], [-R, -R, 0], [R, R, h], 10)


def c12_mirror_feature():
    r, hb = 6.0, 15.0
    mp = MP().add(*box_mp([-30, 0, 0], [30, 30, 10]))
    for x in (15, -15):
        mp.add(PI * r * r * hb, [x, 15, 10 + hb / 2])
    A = box_area(60, 30, 10) + 2 * (2 * PI * r * hb)
    return result("block [-30,30]x[0,30]x[0,10] + two bosses r=6 h=15 at (+-15,15) on top (mirror across YZ): "
                  "V = abc + 2 pi r^2 h, A = 2(ab+bc+ca) + 2 * 2 pi r h", 1, mp.v, A, mp.com(), [-30, 0, 0], [30, 30, 25], 10)


def c13_combine_join():
    r = 12.0
    mp = MP().add(*box_mp([0, 0, 0], [40, 40, 20]))
    mp.add(PI * r * r / 2 * 20, [40 + 4 * r / (3 * PI), 20, 10])  # half cylinder x>40, z 0..20
    mp.add(PI * r * r * 20, [40, 20, 30])  # full cylinder z 20..40
    A = (1600 + PI * r * r / 2) + (1600 - PI * r * r / 2) + PI * r * r + 3 * 800 + (800 - 2 * r * 20) + PI * r * 40 + PI * r * 20
    return result("box [0,40]^2x[0,20] union cylinder r=12 at (40,20) z 0..40: V = box + half-cylinder (x>40, z<20, centroid "
                  "40 + 4r/3pi) + full cylinder z 20..40; A = bottom (1600 + pi r^2/2) + box top (1600 - pi r^2/2) + cyl top + "
                  "3 full sides + x=40 side minus 24x20 + cyl side (pi r 40 outside half + pi r 20 upper inside half)",
                  1, mp.v, A, mp.com(), [0, 0, 0], [52, 40, 40], 9)


def c14_combine_cut():
    r = 15.0
    mp = MP().add(*box_mp([-25, -25, -20], [25, 25, 0]))
    mp.add(2 / 3 * PI * r ** 3, [0, 0, -3 * r / 8], -1)
    A = box_area(50, 50, 20) - PI * r * r + 2 * PI * r * r
    return result("box [-25,25]^2x[-20,0] minus sphere r=15 at the origin (lower hemisphere removed): V = abc - 2/3 pi r^3, "
                  "hemisphere centroid z = -3r/8; A = 2(ab+bc+ca) - pi r^2 + 2 pi r^2", 1, mp.v, A, mp.com(),
                  [-25, -25, -20], [25, 25, 0], 7)


def c15_combine_intersect():
    R, a = 25.0, 20.0
    h = R - a
    rc = math.sqrt(R * R - a * a)
    V = 4 / 3 * PI * R ** 3 - 6 * cap_v(R, h)
    A = 4 * PI * R * R - 6 * 2 * PI * R * h + 6 * PI * rc * rc
    return result("cube [-20,20]^3 intersect sphere R=25: sphere minus 6 disjoint caps of height h=5 (caps cannot overlap: "
                  "two would need x^2+y^2 > 800 > R^2); V = 4/3 pi R^3 - 6 pi h^2 (3R-h)/3, A = 4 pi R^2 - 6 * 2 pi R h + 6 pi (R^2-20^2)",
                  1, V, A, [0, 0, 0], [-a, -a, -a], [a, a, a], 7)


def c16_sweep():
    r, R, L1, L2 = 3.0, 20.0, 30.0, 30.0
    th = PI / 2
    ar = PI * r * r
    mp = MP()
    mp.add(ar * L1, [0, 0, L1 / 2])  # vertical leg z 0..30 along the Z axis
    # quarter torus: bend centre C=(20, 30) in XZ, from direction (-1,0) to (0,1) about C
    Vt = ar * R * th
    c_eff = (R * R + r * r / 4) / R  # int rho^2 dA / int rho dA over the section disk
    rho = c_eff * math.sin(th / 2) / (th / 2)  # sector centroid distance from the bend axis
    bis = (-math.sqrt(0.5), math.sqrt(0.5))
    mp.add(Vt, [20 + rho * bis[0], 0, 30 + rho * bis[1]])
    mp.add(ar * L2, [20 + L2 / 2, 0, 50])
    A = 2 * PI * r * (L1 + R * th + L2) + 2 * ar
    return result("circle r=3 swept along line 30 (+Z) + tangent R20 quarter arc + line 30 (+X): two cylinders + quarter torus "
                  "(Pappus: V = pi r^2 R theta, A = 2 pi r R theta; torus centroid at rho = (R^2 + r^2/4)/R * sin(theta/2)/(theta/2) "
                  "from the bend axis along the bisector) + 2 end disks", 1, mp.v, A, mp.com(), [-r, -r, 0], [50, r, 50 + r], 5)


def c17_loft():
    """A ruled loft from the 40 x 40 square at z = 0 to the circle r 10 at z = 40. Each side of the
    square runs to one quarter of the circle, the corner (20, -20) to angle -45 deg, and both by
    their own parameters: the side linearly, the quarter as the exact rational quadratic arc
    (middle weight sqrt(2) / 2) that is the circle's B-spline form. Point (u, t) of a wall is
    (1 - t) S(u) + t C(u) at height 40 t, so the section at t is the blend of the two loops and
    its area A(t) = (1 - t)^2 A_S + t^2 A_C + t (1 - t) M with M the mixed area
    1/2 int (S x C' + C x S') du."""
    h, half, r = 40.0, 20.0, 10.0
    w = math.sqrt(2.0) / 2.0
    corners = [(half, -half), (half, half), (-half, half), (-half, -half)]

    def side(k, t):
        (ax, ay), (bx, by) = corners[k], corners[(k + 1) % 4]
        return (ax + (bx - ax) * t, ay + (by - ay) * t), (bx - ax, by - ay)

    def quarter(k, t):
        a0 = -PI / 4 + k * PI / 2
        p0 = (r * math.cos(a0), r * math.sin(a0))
        p2 = (r * math.cos(a0 + PI / 2), r * math.sin(a0 + PI / 2))
        p1 = (p0[0] + p2[0], p0[1] + p2[1])  # the control polygon's corner, sqrt(2) r out
        b = [(1 - t) ** 2, 2 * t * (1 - t) * w, t * t]
        db = [-2 * (1 - t), 2 * (1 - 2 * t) * w, 2 * t]
        d = sum(b)
        dd = sum(db)
        n = [sum(bi * q[i] for bi, q in zip(b, (p0, p1, p2))) for i in range(2)]
        dn = [sum(bi * q[i] for bi, q in zip(db, (p0, p1, p2))) for i in range(2)]
        return (n[0] / d, n[1] / d), ((dn[0] * d - n[0] * dd) / (d * d), (dn[1] * d - n[1] * dd) / (d * d))

    xs, ws = GL
    a_s, a_c = (2 * half) ** 2, PI * r * r
    mixed = 0.0
    area_walls = 0.0
    for k in range(4):
        for x, wu in zip(xs, ws):
            u = 0.5 * (x + 1)
            (s, ds), (c, dc) = side(k, u), quarter(k, u)
            mixed += 0.5 * wu * 0.5 * ((s[0] * dc[1] - s[1] * dc[0]) + (c[0] * ds[1] - c[1] * ds[0]))
            for y, wt in zip(xs, ws):
                t = 0.5 * (y + 1)
                pu = ((1 - t) * ds[0] + t * dc[0], (1 - t) * ds[1] + t * dc[1], 0.0)
                pt = (c[0] - s[0], c[1] - s[1], h)
                n = (pu[1] * pt[2] - pu[2] * pt[1], pu[2] * pt[0] - pu[0] * pt[2], pu[0] * pt[1] - pu[1] * pt[0])
                area_walls += 0.25 * wu * wt * math.sqrt(sum(v * v for v in n))
    int_a = a_s / 3 + a_c / 3 + mixed / 6
    int_ta = a_s / 12 + a_c / 4 + mixed / 12
    V = h * int_a
    zc = h * int_ta / int_a
    return result(
        "ruled loft, square 40 at z 0 to circle r 10 at z 40, each side on one rational quarter of the circle "
        "(corner (20, -20) at -45 deg): A(t) = (1-t)^2 A_S + t^2 A_C + t(1-t) M, M = 1/2 int (S x C' + C x S') du "
        "(Gauss-Legendre on the exact rational arcs), V = h (A_S/3 + A_C/3 + M/6), z_c = h (A_S/12 + A_C/4 + M/12) / (V/h), "
        "wall area = int |P_u x P_t| du dt",
        1,
        V,
        area_walls + a_s + a_c,
        (0.0, 0.0, zc),
        (-half, -half, 0.0),
        (half, half, h),
        faces=6,
        note="a difference of definition with the FreeCAD (OpenCASCADE) loft, which runs each side to the quarter at "
        "uniform angle (arc length): that correspondence has no exact rational B-spline form, and SolidCraft keeps its "
        "surfaces exact; OpenCASCADE's volume is 35125.50 mm^3 (0.13% more). The reference.stl is OpenCASCADE's, so "
        "the surface comparison is left out",
        compare=["body_count", "volume_mm3", "area_mm2", "center_of_mass", "bbox", "faces"],
    )


def c18_hole_features():
    T = 15.0
    mp = MP().add(*box_mp([0, 0, 0], [60, 40, T]))
    A = box_area(60, 40, T)
    # counterbore at (15,20): r=7 depth 5, then r=4 through
    mp.add(PI * 49 * 5, [15, 20, T - 2.5], -1)
    mp.add(PI * 16 * 10, [15, 20, 5], -1)
    A += -PI * 49 + 2 * PI * 7 * 5 + PI * (49 - 16) + 2 * PI * 4 * 10 - PI * 16
    # countersink at (35,20): 90 deg cone r=6 at the top to r=3 at depth 3, then r=3 through
    hc = (6 - 3) / math.tan(math.radians(45))
    mp.add(frustum_v(hc, 3, 6), [35, 20, T - hc + frustum_zc(hc, 3, 6)], -1)
    mp.add(PI * 9 * (T - hc), [35, 20, (T - hc) / 2], -1)
    A += -PI * 36 + frustum_lat(hc, 3, 6) + 2 * PI * 3 * (T - hc) - PI * 9
    # blind drilled hole at (50,20): r=3 to depth 10 (shoulder), 118 deg point below
    tip = 3 / math.tan(math.radians(59))
    zs = T - 10
    mp.add(PI * 9 * 10, [50, 20, zs + 5], -1)
    mp.add(PI * 9 * tip / 3, [50, 20, zs - tip / 4], -1)
    A += -PI * 9 + 2 * PI * 3 * 10 + PI * 3 * math.hypot(3, tip)
    return result("plate 60x40x15 minus: counterbore (r7 x 5 deep + r4 through), countersink (90 deg cone frustum r6->r3 over 3 mm "
                  "+ r3 through), blind r3 hole 10 deep to the shoulder with a 118 deg cone point (height 3/tan59, centroid 1/4 up "
                  "from the apex); areas: removed top/bottom disks, cylinder walls 2 pi r h, cb floor annulus, cone laterals pi r s",
                  1, mp.v, A, mp.com(), [0, 0, 0], [60, 40, T], 13)


def c19_l_bracket():
    # solved: l1 (0,0)-(60,0), l2 up 8, l3 left to x=8 (l5 is 8 long and l6 is vertical at x=0), l4 up to y=40, l5 to (0,40)
    h = 20.0
    mp = MP().add(*box_mp([0, 0, 0], [60, 8, h])).add(*box_mp([0, 8, 0], [8, 40, h]))
    a2 = 60 * 8 + 8 * 32
    A = 2 * a2 + (60 + 8 + 52 + 32 + 8 + 40) * h
    return result("solved L: (0,0)-(60,0)-(60,8)-(8,8)-(8,40)-(0,40); area 60x8 + 8x32, extruded 20: V = A2 h, "
                  "A = 2 A2 + perimeter*h, centroid of the two rectangles", 1, mp.v, A, mp.com(), [0, 0, 0], [60, 40, h], 8)


def c20_slot():
    r, L, h = 6.0, 40.0, 5.0
    a2 = 2 * r * L + PI * r * r
    per = 2 * L + 2 * PI * r
    return result("obround: arc centres (0,0) and (40,0), R6, tangent horizontal lines y=+-6: A2 = 2rL + pi r^2, "
                  "V = A2 h, A = 2 A2 + perimeter (2L + 2 pi r) h", 1, a2 * h, 2 * a2 + per * h, [L / 2, 0, h / 2],
                  [-r, -r, 0], [L + r, r, h], 6)


def c21_draft():
    h, a0, t = 30.0, 40.0, math.radians(5)
    a1 = a0 + 2 * h * math.tan(t)  # pull = -Z (bottom normal); faces lean in along the pull, so the top widens
    V, zc = sq_frustum(h, a0, a1)
    A = a0 * a0 + a1 * a1 + 4 * (a0 + a1) / 2 * h / math.cos(t)
    return result("40x40x30 box, 5 deg draft hinged on the bottom (z=0), pull -Z: square frustum 40 -> 40 + 2*30*tan5; "
                  "V = h/3 (A0 + A1 + sqrt(A0A1)), sides 4 trapezoids of slant h/cos5", 1, V, A, [0, 0, zc],
                  [-a1 / 2, -a1 / 2, 0], [a1 / 2, a1 / 2, h], 6)


def c22_split_body():
    r, h = 20.0, 60.0
    t = math.radians(30)
    V = PI * r * r * h
    A = 2 * PI * r * r + 2 * PI * r * h + 2 * PI * r * (r / math.cos(t))
    return result("cylinder r20 z -30..30 split by the plane through the X axis tilted 30 deg (z = y tan30, stays within |z|<=11.6 "
                  "inside the cylinder): 2 congruent halves (point symmetry), V total = pi r^2 h, A total = cylinder + 2 ellipses "
                  "pi r (r/cos30)", 2, V, A, [0, 0, 0], [-r, -r, -h / 2], [r, r, h / 2], 6)


def c24_user_parameters():
    w, d, h, rh = 60.0, 40.0, 15.0, 5.0
    V = (w * d - PI * rh * rh) * h
    A = 2 * (w * d - PI * rh * rh) + 2 * (w + d) * h + 2 * PI * rh * h
    return result("width 60 x depth 40 x height 15 block from the origin with a centred d10 through hole: V = (wd - pi r^2) h, "
                  "A = 2(wd - pi r^2) + 2(w+d)h + 2 pi r h", 1, V, A, [w / 2, d / 2, h / 2], [0, 0, 0], [w, d, h], 7)


def c25_sketch_triangle():
    # P = t (cos60, sin60), |P - (50,0)| = 45 -> t^2 - 50 t + 475 = 0; the drawn apex (|P| ~ 41) picks t = 25 + sqrt(150)
    tt = 25 + math.sqrt(150)
    P = (tt * 0.5, tt * math.sqrt(3) / 2)
    h = 5.0
    a2 = 0.5 * 50 * P[1]
    per = 50 + 45 + tt
    return result("solved triangle (0,0), (50,0), P = t(cos60, sin60) with t = 25 + sqrt(150) from |P-(50,0)| = 45 "
                  "(the root nearest the drawn apex); V = (1/2 * 50 * Py) * 5, A = 2 A2 + perimeter*5, centroid = vertex mean",
                  1, a2 * h, 2 * a2 + per * h, [(50 + P[0]) / 3, P[1] / 3, h / 2], [0, 0, 0], [50, P[1], h], 5)


def c26_sketch_belt():
    R, r, d, rh, h = 20.0, 10.0, 50.0, 4.0, 5.0
    al = math.asin((R - r) / d)
    nt = (math.sin(al), math.cos(al))
    nb = (math.sin(al), -math.cos(al))
    b1, b2 = (R * nb[0], R * nb[1]), (d + r * nb[0], r * nb[1])
    t2, t1 = (d + r * nt[0], r * nt[1]), (R * nt[0], R * nt[1])
    segs = [("line", b1, b2), ("arc", (d, 0), r, -(PI / 2 - al), PI / 2 - al), ("line", t2, t1),
            ("arc", (0, 0), R, PI / 2 - al, 3 * PI / 2 + al)]
    A2, Sx, Sy, _ = region(segs)
    per_out = sum(seg_len(s) for s in segs)
    A2n = A2 - PI * rh * rh
    Sxn = Sx - PI * rh * rh * d
    V = A2n * h
    A = 2 * A2n + (per_out + 2 * PI * rh) * h
    return result("belt: convex hull of circles R20 @ (0,0) and r10 @ (50,0) (external tangents, sin a = (R-r)/d) minus the "
                  "d8 hole at (50,0), extruded 5 as one body; hull area = (R+r) sqrt(d^2-(R-r)^2) + R^2(pi+2a)/2 + r^2(pi-2a)/2; "
                  "moments by Green's theorem over the line/arc boundary", 1, V, A, [Sxn / A2n, 0, h / 2],
                  [-R, -R, 0], [d + r, R, h], 7)


def c27_sketch_trapezoid():
    a, b, H, rh, t = 60.0, 30.0, 25.0, 3.0, 5.0
    At = (a + b) / 2 * H
    yt = H * (a + 2 * b) / (3 * (a + b))
    A2 = At - 2 * PI * rh * rh
    yc = (At * yt - 2 * PI * rh * rh * 8) / A2
    per = a + b + 2 * math.hypot((a - b) / 2, H) + 2 * 2 * PI * rh
    return result("solved trapezoid bottom (-30,0)-(30,0), top (-15,25)-(15,25) minus two d6 holes at (+-18, 8) "
                  "(symmetric about the axis, 36 apart, 8 above l1); trapezoid centroid y = H(a+2b)/(3(a+b)); "
                  "V = A2 t, A = 2 A2 + perimeter t", 1, A2 * t, 2 * A2 + per * t, [0, yc, t / 2], [-30, 0, 0], [30, H, t], 8)


def c29_two_sided_partial_revolve():
    # block: x -20..20, z 0..20 (XZ sketch, y_dir = -Z), extruded +10 along +Y and 25 along -Y
    mp = MP().add(*box_mp([-20, -25, 0], [20, 10, 20]))
    ri, ro, z0, z1 = 4.0, 12.0, 12.0, 20.0
    th = 1.5 * PI  # sweeps from +X towards the sketch normal (+Y): theta in [0, 270 deg]
    # removed cross-section S = annular sector theta in [0, th] minus the part above y = 10 (outside the block)
    Asec = th / 2 * (ro * ro - ri * ri)
    Mx_sec = (ro ** 3 - ri ** 3) / 3 * math.sin(th)
    My_sec = (ro ** 3 - ri ** 3) / 3 * (1 - math.cos(th))
    dy = 10.0
    seg_a = ro * ro * math.acos(dy / ro) - dy * math.sqrt(ro * ro - dy * dy)
    seg_my = 2 / 3 * (ro * ro - dy * dy) ** 1.5
    S = Asec - seg_a
    Mx, My = Mx_sec, My_sec - seg_my
    hz = z1 - z0
    mp.add(S * hz, [Mx / S, My / S, (z0 + z1) / 2], -1)
    half_w = math.sqrt(ro * ro - dy * dy)
    phi = math.asin(dy / ro)
    outer_ang = th - (PI - 2 * phi)
    A = box_area(40, 35, 20) - S + S - 2 * half_w * hz + ri * th * hz + ro * outer_ang * hz + 2 * (ro - ri) * hz
    return result("block [-20,20]x[-25,10]x[0,20] minus the 270 deg revolve (from +X towards +Y) of the 8x8 square r 4..12, z 12..20; "
                  "the removed section is the annular sector minus the circular segment y > 10 (R^2 acos(d/R) - d sqrt(R^2-d^2), "
                  "y-moment 2/3 (R^2-d^2)^1.5) that lies outside the block; sector moments (R^3-r^3)/3 (sin, 1-cos); "
                  "A = box - top opening + floor - front-face notch 2 sqrt(44) x 8 + inner wall r th h + outer wall (th - pi + 2 asin(5/6)) R h + 2 end caps",
                  1, mp.v, A, mp.com(), [-20, -25, 0], [20, 10, 20], 12)


def c82_thin_extrude_frame():
    a, t_in, t_out, h = 40.0, 3.0, 1.0, 10.0
    outer, inner = a + 2 * t_out, a - 2 * t_in
    ring = outer * outer - inner * inner
    return result("square frame: the lines' 40 mm square walled 3 mm inside and 1 mm outside (mitred corners) is "
                  "42 x 42 less 34 x 34, 10 mm tall: V = (42^2 - 34^2) * 10, A = 2 (42^2 - 34^2) + 4 * 42 * 10 + 4 * 34 * 10",
                  1, ring * h, 2 * ring + 4 * outer * h + 4 * inner * h, [0, 0, h / 2], [-outer / 2, -outer / 2, 0],
                  [outer / 2, outer / 2, h], 10)


CASES = {
    "01-box": c01_box,
    "02-cylinder": c02_cylinder,
    "03-box-through-hole": c03_box_through_hole,
    "04-revolve-vase": c04_revolve_vase,
    "05-taper-extrude": c05_taper_extrude,
    "06-symmetric-extrude": c06_symmetric_extrude,
    "07-fillet-all-edges": c07_fillet_all_edges,
    "08-chamfer": c08_chamfer,
    "09-shell": c09_shell,
    "10-rect-pattern-holes": c10_rect_pattern_holes,
    "11-circular-pattern-flange": c11_circular_pattern_flange,
    "12-mirror-feature": c12_mirror_feature,
    "13-combine-join": c13_combine_join,
    "14-combine-cut": c14_combine_cut,
    "15-combine-intersect": c15_combine_intersect,
    "16-sweep": c16_sweep,
    "17-loft": c17_loft,
    "18-hole-features": c18_hole_features,
    "19-l-bracket-constrained": c19_l_bracket,
    "20-slot-constrained": c20_slot,
    "21-draft": c21_draft,
    "22-split-body": c22_split_body,
    "24-user-parameters": c24_user_parameters,
    "25-sketch-triangle": c25_sketch_triangle,
    "26-sketch-belt": c26_sketch_belt,
    "27-sketch-trapezoid": c27_sketch_trapezoid,
    "29-two-sided-and-partial-revolve": c29_two_sided_partial_revolve,
    "82-thin-extrude-frame": c82_thin_extrude_frame,
}


def main(argv):
    names = argv[1:] or list(CASES)
    for n in names:
        d = CASES[n]()
        path = os.path.join(ROOT, n, "expected.json")
        with open(path, "w") as f:
            json.dump(d, f, indent=2)
            f.write("\n")
        print(f"{n}: V={d['volume_mm3']!r} A={d['area_mm2']!r} com={d['center_of_mass']}")


if __name__ == "__main__":
    main(sys.argv)
