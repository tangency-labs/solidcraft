#!/usr/bin/env python3
"""Closed-form expected values for the feature cases of tests/oracle (analytic oracle).

Standalone, Python 3 stdlib only. Run from the worktree root:

    python3 tools/oracle-analytic/cases_features.py

Every number is derived from the recipe's dimensions with elementary geometry:
prisms, cylinders, cones, spheres, tori, solids of revolution (Pappus / Green's theorem on
the (r, z) profile), constant fillets as swept spandrels, tubes, screw sweeps.  Planar
regions bounded by lines and circular arcs are integrated exactly with Green's theorem
(Gauss-Legendre on each segment, exact to round-off for lines and short arc pieces).
"""

import json
import math
import os

PI = math.pi
ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..")
OUT = os.path.join(ROOT, "tests", "oracle")

# --------------------------------------------------------------------------------------
# Planar regions (lines + circular arcs), Green's theorem.
# --------------------------------------------------------------------------------------


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


GL_X, GL_W = _gauss_legendre(24)


def L(p, q):
    """Line segment p -> q."""
    return ("L", tuple(p), tuple(q))


def A(c, r, a0, a1):
    """Arc about centre c, radius r, from angle a0 to a1 (radians; a1 < a0 is clockwise)."""
    return ("A", tuple(c), r, a0, a1)


def _pieces(seg):
    """Parametric pieces (point(t), derivative(t)) over t in [0, 1]."""
    if seg[0] == "L":
        (x0, y0), (x1, y1) = seg[1], seg[2]
        yield (lambda t: (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t), lambda t: (x1 - x0, y1 - y0))
    else:
        (cx, cy), r, a0, a1 = seg[1], seg[2], seg[3], seg[4]
        n = max(1, int(math.ceil(abs(a1 - a0) / (PI / 36))))
        for k in range(n):
            b0 = a0 + (a1 - a0) * k / n
            b1 = a0 + (a1 - a0) * (k + 1) / n
            db = b1 - b0
            yield (
                (lambda b0, db: lambda t: (cx + r * math.cos(b0 + db * t), cy + r * math.sin(b0 + db * t)))(b0, db),
                (lambda b0, db: lambda t: (-r * db * math.sin(b0 + db * t), r * db * math.cos(b0 + db * t)))(b0, db),
            )


def region(loops):
    """Integrals of a region whose boundary loops are CCW (outer) / CW (holes).

    Returns dict: A, Sx = int x dA, Sy = int y dA, Sxy = int x y dA, P = perimeter,
    Px = int x ds over the boundary.
    """
    acc = dict(A=0.0, Sx=0.0, Sy=0.0, Sxy=0.0, P=0.0, Px=0.0)
    for loop in loops:
        for seg in loop:
            for pt, dpt in _pieces(seg):
                for xg, wg in zip(GL_X, GL_W):
                    t = 0.5 * (xg + 1)
                    w = 0.5 * wg
                    x, y = pt(t)
                    dx, dy = dpt(t)
                    ds = math.hypot(dx, dy)
                    acc["A"] += w * x * dy
                    acc["Sx"] += w * x * x / 2 * dy
                    acc["Sy"] += w * x * y * dy
                    acc["Sxy"] += w * x * x * y / 2 * dy
                    acc["P"] += w * ds
                    acc["Px"] += w * x * ds
    return acc


def poly(pts):
    return [L(pts[i], pts[(i + 1) % len(pts)]) for i in range(len(pts))]


def reverse(loop):
    out = []
    for seg in reversed(loop):
        out.append(L(seg[2], seg[1]) if seg[0] == "L" else A(seg[1], seg[2], seg[4], seg[3]))
    return out


def circle_loop(c, r, ccw=True):
    return [A(c, r, 0.0, 2 * PI if ccw else -2 * PI)]


# --------------------------------------------------------------------------------------
# Mass properties: (V, [Mx, My, Mz]) with composition.
# --------------------------------------------------------------------------------------


class MP:
    def __init__(self, v=0.0, m=(0.0, 0.0, 0.0)):
        self.v = v
        self.m = list(m)

    @staticmethod
    def at(v, c):
        return MP(v, [v * c[0], v * c[1], v * c[2]])

    def __add__(self, o):
        return MP(self.v + o.v, [a + b for a, b in zip(self.m, o.m)])

    def __sub__(self, o):
        return MP(self.v - o.v, [a - b for a, b in zip(self.m, o.m)])

    def com(self):
        return [x / self.v for x in self.m]


def box(x0, y0, z0, x1, y1, z1):
    v = (x1 - x0) * (y1 - y0) * (z1 - z0)
    return MP.at(v, [(x0 + x1) / 2, (y0 + y1) / 2, (z0 + z1) / 2])


def cyl(cx, cy, r, z0, z1):
    return MP.at(PI * r * r * (z1 - z0), [cx, cy, (z0 + z1) / 2])


def extrude(reg, z0, h):
    """Prism of a planar region (sketch x, y = world x, y) from z0 to z0 + h."""
    return MP(reg["A"] * h, [reg["Sx"] * h, reg["Sy"] * h, reg["A"] * h * (z0 + h / 2)])


def extrude_area(reg, h):
    return 2 * reg["A"] + reg["P"] * h


def revolve(reg, cx=0.0, cy=0.0):
    """Full revolution about the vertical axis through (cx, cy) of an (r, z) region (r >= 0)."""
    v = 2 * PI * reg["Sx"]
    return MP(v, [v * cx, v * cy, 2 * PI * reg["Sxy"]])


def revolve_area(reg):
    """Area of the surface swept by the whole boundary (segments on the axis add nothing)."""
    return 2 * PI * reg["Px"]


# Spandrel of a fillet radius R: the square corner R x R minus the quarter disc.
def spandrel_area(R):
    return R * R * (1 - PI / 4)


def spandrel_offset(R):
    """Distance of the spandrel's centroid from each of the two corner faces."""
    return R * (10 - 3 * PI) / (3 * (4 - PI))


# --------------------------------------------------------------------------------------
# Output
# --------------------------------------------------------------------------------------


def write(case, method, bodies, mp, area, bbox, faces=None, **extra):
    path = os.path.join(OUT, case, "expected.json")
    d = {"source": "analytic"}
    # known_failure is kept by hand (the oracle fails the job when a known failure passes), so
    # regenerating the values keeps it
    try:
        with open(path) as f:
            old = json.load(f)
        if "known_failure" in old:
            d["known_failure"] = old["known_failure"]
    except (OSError, ValueError):
        pass
    d.update({
        "method": method,
        "body_count": bodies,
        "volume_mm3": mp.v,
        "area_mm2": area,
        "center_of_mass": mp.com(),
        "bbox": {"min": list(map(float, bbox[0])), "max": list(map(float, bbox[1]))},
    })
    if faces is not None:
        d["faces"] = faces
    d.update(extra)
    with open(path, "w") as f:
        json.dump(d, f, indent=2)
        f.write("\n")
    print(f"{case}: V={mp.v:.6f} A={area:.6f} com={[round(x, 6) for x in mp.com()]}")


# --------------------------------------------------------------------------------------
# Cases
# --------------------------------------------------------------------------------------


def c32():
    # Plate 60x60x8 centred; boss r10 z 8..23 with R3 concave base fillet and R2 convex top fillet.
    # The boss with both fillets is one solid of revolution: its (r, z) profile.
    prof = region([[
        L((0, 8), (13, 8)),
        A((13, 11), 3, -PI / 2, -PI),           # concave base fillet, (13,8) -> (10,11)
        L((10, 11), (10, 21)),
        A((8, 21), 2, 0.0, PI / 2),             # convex top fillet, (10,21) -> (8,23)
        L((8, 23), (0, 23)),
        L((0, 23), (0, 8)),
    ]])
    mp = box(-30, -30, 0, 30, 30, 8) + revolve(prof)
    # Revolved boundary minus its bottom disc (r 0..13 at z=8, internal); plate top loses that disc.
    area = (3600 + 4 * 60 * 8 + 3600 - PI * 13 ** 2) + (revolve_area(prof) - PI * 13 ** 2)
    write("32-boss-plate-fillets",
          "plate 60x60x8 + boss (r10, z 8..23) with R3 concave base and R2 convex top fillets taken as one "
          "solid of revolution: V, z-moment and lateral area from Pappus/Green on its (r,z) profile "
          "(quarter-circle fillet arcs); plate top loses the r13 disc",
          1, mp, area, [(-30, -30, 0), (30, 30, 23)], faces=10)


def c33():
    # Obround R8 (centres x=0, x=40), height 10.  Top loop R2 fillet, bottom loop 1 mm chamfer.
    R, Lst, H = 8.0, 40.0, 10.0
    rf, ch = 2.0, 1.0
    base = MP.at((2 * R * Lst + PI * R * R) * H, [Lst / 2, 0, H / 2])

    def path_len(inset):  # perimeter of the obround inset by `inset`
        return 2 * Lst + 2 * PI * (R - inset)

    # The perimeter of the inward offset shrinks as P(u) = P(0) - 2*pi*u.
    # Pappus gives exact swept volumes, but using the unweighted section
    # centroid for Z misses the correlation between inset u and depth v.
    perimeter = path_len(0)
    d = spandrel_offset(rf)
    fillet_area = spandrel_area(rf)
    fillet_volume = fillet_area * path_len(d)
    # Mixed first moment int(u*v dA) of square r*r minus quarter disk.
    fillet_inset_depth_moment = rf ** 4 * (19 / 24 - PI / 4)
    fillet_depth_moment = (perimeter * fillet_area * d
                           - 2 * PI * fillet_inset_depth_moment)
    fil = MP.at(fillet_volume, [Lst / 2, 0,
                               H - fillet_depth_moment / fillet_volume])
    # For the bottom chamfer, int(z dV) = P*c^3/6 - pi*c^4/12.
    chamfer_volume = ch * ch / 2 * path_len(ch / 3)
    chamfer_z_moment = perimeter * ch ** 3 / 6 - PI * ch ** 4 / 12
    cha = MP.at(chamfer_volume, [Lst / 2, 0,
                                chamfer_z_moment / chamfer_volume])
    mp = base - fil - cha
    top = 2 * (R - rf) * Lst + PI * (R - rf) ** 2
    bot = 2 * (R - ch) * Lst + PI * (R - ch) ** 2
    wall = (H - rf - ch) * path_len(0)
    # fillet surface: quarter arc length pi*rf/2, its centroid inset rf - 2rf/pi from the wall
    fsurf = PI * rf / 2 * path_len(rf - 2 * rf / PI)
    csurf = math.sqrt(2) * ch * path_len(ch / 2)
    area = top + bot + wall + fsurf + csurf
    write("33-fillet-tangent-chain",
          "obround (R8, centres 40 apart) x 10 minus R2 fillet spandrel R^2(1-pi/4) and 1 mm chamfer "
          "triangle swept round the whole loop. Swept volumes use Pappus; Z moments "
          "account for inward-offset perimeter P(u)=P-2pi*u: fillet top-depth moment "
          "P*A*d-2pi*r^4*(19/24-pi/4), A=r^2*(1-pi/4), "
          "d=r*(10-3pi)/(3*(4-pi)); bottom chamfer Z moment "
          "P*c^3/6-pi*c^4/12; surfaces use fillet arc centroid inset r-2r/pi",
          1, mp, area, [(-8, -8, 0), (48, 8, 10)], faces=14)


def c38():
    mp = cyl(0, 0, 10, 0, 50)
    area = 2 * PI * 10 * 50 + 2 * PI * 100
    write("38-coincident-cylinders",
          "union of r10 z0..30 and r10 z20..50 = cylinder r10 h50: V = pi r^2 h, A = 2 pi r h + 2 pi r^2",
          1, mp, area, [(-10, -10, 0), (10, 10, 50)], faces=3)


def c40():
    r_s, R_t, r_t = 12.0, 22.0, 4.0
    b = box(-30, -30, -10, 30, 30, 0)
    hemi = MP.at(2 / 3 * PI * r_s ** 3, [0, 0, 3 * r_s / 8])
    # The sphere (r12) also pokes 2 mm out of the box bottom (z=-10): a spherical cap h=2.
    h = r_s - 10.0
    cap = MP.at(PI * h * h * (3 * r_s - h) / 3, [0, 0, -3 * (2 * r_s - h) ** 2 / (4 * (3 * r_s - h))])
    # Torus centred on the box top: only its lower half lies in material.
    half_torus = MP.at(PI * r_t ** 2 / 2 * 2 * PI * R_t, [0, 0, -4 * r_t / (3 * PI)])
    mp = b + hemi + cap - half_torus
    area = (3600 - PI * (r_s ** 2 - 10.0 ** 2) + 4 * 60 * 10   # bottom (minus cap footprint) + sides
            + 3600 - PI * r_s ** 2 - PI * ((R_t + r_t) ** 2 - (R_t - r_t) ** 2)  # top
            + 2 * PI * r_s ** 2                          # upper hemisphere
            + 2 * PI * r_s * h                           # cap below the box
            + 2 * PI * R_t * PI * r_t)                   # half torus (semicircle r_t about R_t)
    write("40-box-sphere-torus",
          "box 60x60x10 (z -10..0) + upper hemisphere r12 + spherical cap h2 below z=-10 (V = pi h^2(3R-h)/3, "
          "A = 2 pi R h) - lower half torus R22 r4 (Pappus: V = pi r^2/2 * 2 pi R, centroid -4r/3pi, "
          "A = pi r * 2 pi R); top loses disc r12 and annulus 18..26, bottom loses disc r sqrt(44)",
          1, mp, area, [(-30, -30, -12), (30, 30, 12)], faces=10,
          note="faces: bottom, 4 sides, top split by the groove into an annulus (r 12..18) and the outer part, "
               "upper sphere, lower cap, torus")


def c43():
    mp = box(0, 0, 0, 35, 20, 20) + box(45, 0, 0, 80, 20, 20)
    area = 2 * 2 * (35 * 20 + 20 * 20 + 35 * 20)
    write("43-cut-splits-body",
          "two boxes 35x20x20 (x 0..35 and 45..80): V = 2abc, A = 2*2(ab+bc+ca)",
          2, mp, area, [(0, 0, 0), (80, 20, 20)], faces=12)


def c44():
    # L (x 0..60, y 0..8 and x 0..8, y 0..40) x 20; shell 2 inward, top open.
    # SolidCraft's shell convention (intersection join): the inner walls are the outer faces
    # offset by 2 and extended until they meet, so the cavity is the L shrunk by 2 with a sharp
    # corner at (6,6) opposite the reflex edge.  Floor at z=2, open at z=20.
    Lpts = [(0, 0), (60, 0), (60, 8), (8, 8), (8, 40), (0, 40)]
    outer = region([poly(Lpts)])
    cav = region([poly([(2, 2), (58, 2), (58, 6), (6, 6), (6, 38), (2, 38)])])
    mp = extrude(outer, 0, 20) - extrude(cav, 2, 18)
    area = (outer["A"] + outer["P"] * 20                # bottom + outer walls
            + (outer["A"] - cav["A"])                    # top rim
            + cav["A"] + cav["P"] * 18)                  # cavity floor + inner walls
    # Rounded (constant-distance, arc) join instead: cavity gains (4-pi) x 18.
    rv, ra = mp.v - (4 - PI) * 18, area - (4 - PI) * 18
    write("44-shell-l-bracket",
          "L prism (area 736, perimeter 200) x 20 minus cavity = L shrunk by 2 (SolidCraft's intersection-join "
          "shell convention: inner faces offset by 2 and extended to meet, sharp inner corner at (6,6); area 352) "
          f"from z=2 to 20. A rounded (arc) join would give V {rv:.4f}, A {ra:.4f}, 16 faces",
          1, mp, area, [(0, 0, 0), (60, 40, 20)], faces=15)


def c45():
    # Plate 60x60x10 centred + boss r12 z 10..30; shell 2 inward, bottom open.
    # Intersection-join convention: cavity = box 56x56 z 0..8 + cylinder r10 z 8..28 (the offset
    # cylinder r10 extended down to meet the offset plate top z=8, sharp circular inner edge).
    solid = box(-30, -30, 0, 30, 30, 10) + cyl(0, 0, 12, 10, 30)
    cav = box(-28, -28, 0, 28, 28, 8) + cyl(0, 0, 10, 8, 28)
    mp = solid - cav
    area = (3600 - 56 * 56                       # bottom rim
            + 4 * 60 * 10                        # plate sides
            + 3600 - PI * 144                    # plate top
            + 2 * PI * 12 * 20 + PI * 144        # boss side + top
            + 4 * 56 * 8                         # cavity side walls
            + 56 * 56 - PI * 100                 # cavity ceiling z=8 (outside r10)
            + 2 * PI * 10 * 20                   # inner cylinder r10 z8..28
            + PI * 100)                          # inner top disc z=28
    # Rounded (arc) join: the ring r10..12, z8..10 outside the r2 torus about (12,10) is also hollow.
    ring = region([[L((10, 8), (12, 8)), A((12, 10), 2, -PI / 2, -PI), L((10, 10), (10, 8))]])
    rv = mp.v - revolve(ring).v
    ra = (area - 2 * PI * 10 * 2 - PI * (144 - 100) + 2 * PI * PI * (12 - 4 / PI))
    write("45-shell-boss-plate",
          "plate 60x60x10 + boss r12 h20 minus cavity (box 56x56x8 + cylinder r10 z8..28) under SolidCraft's "
          "intersection-join shell convention (offset faces extended to meet: sharp circular inner edge at r10, "
          f"z8). A rounded (arc) join would give V {rv:.4f}, A {ra:.4f}, 16 faces (torus round r2)",
          1, mp, area, [(-30, -30, 0), (30, 30, 30)], faces=15)


def c46():
    mp = box(0, 0, 0, 50, 40, 30) - box(3, 0, 3, 47, 37, 30)
    area = (50 * 40 + 50 * 30 + 2 * 40 * 30            # bottom, back, two sides
            + (50 * 40 - 44 * 37) + (50 * 30 - 44 * 27)  # top rim, front rim
            + 44 * 37 + 44 * 27 + 2 * 37 * 27)          # floor, back inner, side inners
    write("46-shell-two-open-faces",
          "box 50x40x30 minus cavity x3..47, y0..37, z3..30 (3 mm walls, top and front open)",
          1, mp, area, [(0, 0, 0), (50, 40, 30)], faces=10)


def c48():
    # Sketch on XZ with y_dir = -Z: profile (r, z) = (sx, -sy).
    pts = [(5, 0), (40, 0), (40, 3), (33, 8), (33, 12), (40, 17), (40, 23), (33, 28), (33, 32),
           (40, 37), (40, 40), (5, 40)]
    reg = region([poly(pts)])
    mp = revolve(reg)
    area = revolve_area(reg)
    write("48-pulley",
          "revolve of the 12-segment (r,z) polygon about Z: V = 2 pi int r dA, Mz = 2 pi int r z dA, "
          "A = 2 pi int r ds (Pappus)",
          1, mp, area, [(-40, -40, 0), (40, 40, 40)], faces=12)


def c49():
    # Coil: the circle r=2 centred at radius 20 in the meridian plane, screw-swept 3 turns, pitch 8.
    Rh, a, p, n = 20.0, 2.0, 8.0, 3.0
    c = p / (2 * PI)
    v = n * 2 * PI * Rh * PI * a * a      # screw motion: only rotation sweeps volume
    # lateral area: n * 2pi * closed-integral sqrt(r^2 + c^2 (dr/ds)^2) ds, r = R + a cos t, dr/ds = -sin t
    N = 4096
    s = 0.0
    for k in range(N):
        t = 2 * PI * k / N
        s += math.sqrt((Rh + a * math.cos(t)) ** 2 + c * c * math.sin(t) ** 2) * a
    lat = n * 2 * PI * s * 2 * PI / N
    area = lat + 2 * PI * a * a
    mp = MP.at(v, [0.0, 0.0, c * n * PI])   # full turns: x,y centroid on the axis; z = c * mean angle
    write("49-helical-sweep",
          "true helix coil (screw motion of the axial d4 section, R20, pitch 8, 3 turns): "
          "V = turns * 2 pi R * pi a^2; lateral A = turns * 2 pi * int sqrt(r^2 + (p/2pi)^2 (dr/ds)^2) ds "
          "(periodic trapezoid, exact to round-off) + two end discs; COM z = pitch*turns/2",
          1, mp, area, [(-22, -22, -2), (22, 22, 26)], faces=3)


def c50():
    r, Rb = 5.0, 5.0
    l1 = cyl(0, 0, r, 0, 20)
    # quarter torus: bend centre (5,0,20), axis Y, from (0,0,20) to (5,0,25).
    vb = PI * r * r * (PI / 2 * Rb)
    rad_mean = (Rb * Rb + r * r / 4) / Rb                 # int rho^2 dA / int rho dA
    off = rad_mean * (math.sin(PI / 4) / (PI / 4)) / math.sqrt(2)
    bend = MP.at(vb, [5 - off, 0, 20 + off])
    l2 = MP.at(PI * r * r * 20, [15, 0, 25])
    mp = l1 + bend + l2
    Lp = 20 + PI / 2 * Rb + 20
    area = 2 * PI * r * Lp + 2 * PI * r * r
    write("50-sweep-tight-bend",
          "tube r5 along line 20 + R5 quarter bend + line 20: V = pi r^2 L, A = 2 pi r L + 2 pi r^2 "
          "(Pappus, valid while bend radius >= r; here equal so the inner side pinches to a line); "
          "bend centroid at (R^2 + r^2/4)/R * sinc(pi/4) along the bisector",
          1, mp, area, [(-5, -5, 0), (25, 5, 30)], faces=5)


def c52():
    disc = cyl(0, 0, 40, 0, 8)
    prof = region([[
        L((0, 8), (8, 8)),
        A((8, 10), 2, -PI / 2, -PI),            # concave fillet R2 at the boss base
        L((6, 10), (6, 18)), L((6, 18), (0, 18)), L((0, 18), (0, 8)),
    ]])
    mp = disc
    for k in range(6):
        th = 2 * PI * k / 6
        mp = mp + revolve(prof, 28 * math.cos(th), 28 * math.sin(th))
    area = (PI * 1600 + 2 * PI * 40 * 8 + PI * 1600
            + 6 * (revolve_area(prof) - 2 * PI * 64))   # boss lateral without its base disc r8; disc top loses it too
    write("52-circular-pattern-fillet",
          "disc r40 h8 + 6 x (boss r6 h10 with R2 concave base fillet as a solid of revolution, Pappus/Green "
          "on its (r,z) profile) at radius 28; disc top loses 6 discs r8",
          1, mp, area, [(-40, -40, 0), (40, 40, 18)], faces=21)


def c53():
    cx, cy = 60.0, 10.0 / 3.0
    R = 45.0 - cy
    th0 = math.atan2(15 - cy, 100 - cx)
    sp = 17.875031
    mp = box(0, 0, 0, 120, 60, 5)
    for k in range(7):
        th = th0 + k * sp / R
        mp = mp - cyl(cx + R * math.cos(th), cy + R * math.sin(th), 3, 0, 5)
    area = 2 * (7200 - 7 * PI * 9) + 2 * (120 + 60) * 5 + 7 * 2 * PI * 3 * 5
    write("53-path-pattern",
          "plate 120x60x5 minus 7 through holes d6 at arc lengths k*17.875031 along the arc through "
          "(100,15),(60,45),(20,15) (centre (60,10/3), R 125/3), translated copies",
          1, mp, area, [(0, 0, 0), (120, 60, 5)], faces=13)


def c54():
    # Mirrored + joined: x -30..30, y 0..40, h10, four R5 vertical corner rounds, holes d8 at (+-15,20).
    r = 5.0
    outer = [
        L((-25, 0), (25, 0)), A((25, 5), r, -PI / 2, 0), L((30, 5), (30, 35)), A((25, 35), r, 0, PI / 2),
        L((25, 40), (-25, 40)), A((-25, 35), r, PI / 2, PI), L((-30, 35), (-30, 5)), A((-25, 5), r, PI, 1.5 * PI),
    ]
    reg = region([outer, circle_loop((15, 20), 4, False), circle_loop((-15, 20), 4, False)])
    mp = extrude(reg, 0, 10)
    area = extrude_area(reg, 10)
    write("54-mirror-body-fillets",
          "prism h10 of the 60x40 rectangle with four R5 corner rounds and two d8 holes at (+-15,20): "
          "V = A h, area = 2A + perimeter*h",
          1, mp, area, [(-30, 0, 0), (30, 40, 10)], faces=12)


def c55():
    r, d, half = 3.4, 12.0, math.radians(59)
    tip = r / math.tan(half)
    mp = (box(0, 0, 0, 40, 30, 15) - cyl(20, 15, r, 15 - d, 15)
          - MP.at(PI * r * r * tip / 3, [20, 15, 15 - d - tip / 4]))
    area = (2 * 1200 + 2 * (40 * 15 + 30 * 15) - PI * r * r
            + 2 * PI * r * d + PI * r * (r / math.sin(half)))
    write("55-tapped-hole-cosmetic",
          "plate 40x30x15 minus d6.8 cylinder 12 deep (to the shoulder) and 118 deg drill point cone "
          "(h = r/tan59, V = pi r^2 h/3, lateral pi r * r/sin59); cosmetic thread adds no geometry",
          1, mp, area, [(0, 0, 0), (40, 30, 15)], faces=8)


def c57():
    r, rc, dc, T = 2.75, 5.0, 5.5, 10.0
    mp = box(0, 0, 0, 80, 50, T)
    for x in (15, 40, 65):
        for y in (15, 35):
            mp = mp - cyl(x, y, rc, T - dc, T) - cyl(x, y, r, 0, T - dc)
    area = (2 * 4000 + 2 * (80 + 50) * T
            + 6 * (-PI * rc * rc - PI * r * r + 2 * PI * rc * dc + PI * (rc * rc - r * r) + 2 * PI * r * (T - dc)))
    write("57-hole-pattern-sketch-points",
          "plate 80x50x10 minus 6 counterbored through holes (cb d10 x 5.5, hole d5.5) at x 15/40/65, y 15/35",
          1, mp, area, [(0, 0, 0), (80, 50, 10)], faces=24)


def c62():
    mp = box(0, 0, 0, 60, 50, 5)
    for k in range(15):
        x0 = 2 + 4 * k
        mp = mp + box(x0, 0, 5, x0 + 1.5, 50, 30)
    area = (3000 + 2 * (60 * 5 + 50 * 5) + 3000 - 15 * 1.5 * 50
            + 15 * (2 * 50 * 25 + 2 * 1.5 * 25 + 1.5 * 50))
    write("62-heat-sink",
          "base 60x50x5 + 15 fins 1.5x50x25 at x = 2 + 4k: sums of boxes; fin ends coplanar with the base ends",
          1, mp, area, [(0, 0, 0), (60, 50, 30)], faces=66,
          note="faces: bottom, x=0, x=60, y=0 and y=50 (each merged with the fin ends), 16 base-top strips, 15x3 fin faces")


def c64():
    # Solved sketch: four R12 arcs on chord 20 (sagitta 12 - sqrt 44) alternating up/down from
    # (0,0) to (80,0) tangent in a chain, closed by (80,0)-(80,-20)-(0,-20)-(0,0).
    R = 12.0
    h = math.sqrt(R * R - 100)         # centre distance from the chord
    al = math.atan2(h, 10)             # half-angle complement
    loop = [
        A((10, -h), R, PI - al, al),           # a1: (0,0) -> (20,0), bulging up (cw)
        A((30, h), R, PI + al, 2 * PI - al),   # a2: (20,0) -> (40,0), bulging down (ccw)
        A((50, -h), R, PI - al, al),           # a3
        A((70, h), R, PI + al, 2 * PI - al),   # a4
        L((80, 0), (80, -20)), L((80, -20), (0, -20)), L((0, -20), (0, 0)),
    ]
    reg = region([reverse(loop)])   # traced clockwise above; reverse to CCW
    mp = extrude(reg, 0, 3)
    area = extrude_area(reg, 3)
    write("64-sketch-tangent-arc-chain",
          "solved sketch: 80x20 rectangle whose top is 4 tangent R12 arcs on 20 mm chords alternating "
          "up/down (net area 1600); prism h3: V = A h, area = 2A + perimeter*h, Green's theorem for centroid",
          1, mp, area, [(0, -20, 0), (80, R - h, 3)], faces=9)


def c66():
    r1, r2, dctr = 12.0, 8.0, 60.0
    al = math.acos((r1 - r2) / dctr)   # direction of the outer tangent normals: +-al
    outer = [
        A((0, 0), r1, al, 2 * PI - al),
        L((r1 * math.cos(al), -r1 * math.sin(al)), (dctr + r2 * math.cos(al), -r2 * math.sin(al))),
        A((dctr, 0), r2, -al, al),
        L((dctr + r2 * math.cos(al), r2 * math.sin(al)), (r1 * math.cos(al), r1 * math.sin(al))),
    ]
    holes = [circle_loop((0, 0), 6, False), circle_loop((60, 0), 4, False),
             circle_loop((30, 4), 2, False), circle_loop((30, -4), 2, False)]
    reg = region([outer] + holes)
    mp = extrude(reg, 0, 6)
    area = extrude_area(reg, 6)
    write("66-sketch-linkage",
          "solved sketch: hull of circles R12 at (0,0) and R8 at (60,0) (outer tangent lines, cos a = 4/60) "
          "minus holes d12, d8 (concentric) and d4 at (30,+-4) (symmetry keeps radii equal); prism h6",
          1, mp, area, [(-12, -12, 0), (68, 12, 6)], faces=10)


CASES = [c32, c33, c38, c40, c43, c44, c45, c46, c48, c49, c50, c52, c53, c54, c55, c57, c62, c64, c66]

if __name__ == "__main__":
    for fn in CASES:
        fn()
