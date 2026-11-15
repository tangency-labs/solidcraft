"""Helpers for the kernel ladder's cases (tests/kernel-cases/ladder): command builders, plain
Python 3 quadrature (standard library only) and the case writers. See cases.py."""
import json
import math
import os

PI = math.pi
ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "tests", "kernel-cases", "ladder")
CASES = []  # (tier, name)
TIERS = {}

def C(cmd, **p):
    return {"command": cmd, "params": p}

def box(corner, l, w, h, **kw):
    return C("solid.box", corner=corner, length=l, width=w, height=h, **kw)

def cyl(base, r, h, **kw):
    return C("solid.cylinder", base=base, radius=r, height=h, **kw)

def sketch(plane, *items, name=None):
    p = {"plane": plane}
    if name:
        p["name"] = name
    return [C("sketch.create", **p), *items, C("sketch.finish")]

def line(points, ids=None, closed=False):
    p = {"points": points}
    if ids:
        p["ids"] = ids
    if closed:
        p["closed"] = True
    return C("sketch.line", **p)

def plane(origin, x, y):
    return {"origin": origin, "x_dir": x, "y_dir": y}

def vplane(origin):
    """A vertical sketch plane: sketch x = world x, sketch y = world z (normal -Y)."""
    return plane(origin, [1, 0, 0], [0, 0, 1])

def flat(*cmds):
    out = []
    for c in cmds:
        out.extend(c if isinstance(c, list) else [c])
    return out

def leggauss(n):
    """Gauss-Legendre nodes and weights on [-1, 1] (Newton on the Legendre polynomial)."""
    xs, ws = [], []
    for i in range(1, n + 1):
        x = math.cos(math.pi * (i - 0.25) / (n + 0.5))
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

_GL = {}

def quad(f, a, b, n=64):
    if n not in _GL:
        _GL[n] = leggauss(n)
    x, w = _GL[n]
    return sum(wi * f(a + (b - a) * (xi + 1) / 2) for xi, wi in zip(x, w)) * (b - a) / 2

def quad_split(f, a, b, pieces=200, n=32):
    h = (b - a) / pieces
    return sum(quad(f, a + i * h, a + (i + 1) * h, n) for i in range(pieces))

def bez(P, t):
    c = ((1 - t) ** 3, 3 * t * (1 - t) ** 2, 3 * t * t * (1 - t), t ** 3)
    return tuple(sum(ci * p[k] for ci, p in zip(c, P)) for k in range(2))

def dbez(P, t):
    c = (3 * (1 - t) ** 2, 6 * t * (1 - t), 3 * t * t)
    d = [tuple(P[i + 1][k] - P[i][k] for k in range(2)) for i in range(3)]
    return tuple(sum(ci * q[k] for ci, q in zip(c, d)) for k in range(2))

def norm(v):
    return math.hypot(*v)

def bez_len(P):
    return quad_split(lambda t: norm(dbez(P, t)), 0, 1, 50)

def bez_min_radius(P):
    best = 1e18
    for i in range(2001):
        t = i / 2000
        d = dbez(P, t)
        dd = tuple(6 * (1 - t) * (P[2][k] - 2 * P[1][k] + P[0][k]) + 6 * t * (P[3][k] - 2 * P[2][k] + P[1][k]) for k in range(2))
        kap = abs(d[0] * dd[1] - d[1] * dd[0]) / norm(d) ** 3
        if kap > 0:
            best = min(best, 1 / kap)
    return best

def bez_area_under(P, base):
    """Area between a Bezier arch (ends on y = base) and the line y = base: -int (y-base) dx."""
    return abs(quad(lambda t: (bez(P, t)[1] - base) * dbez(P, t)[0], 0, 1))

SPANDREL = 1 - PI / 4            # area factor of a fillet's removed section, times r^2
XBAR = (10 - 3 * PI) / (12 - 3 * PI)  # spandrel centroid from the corner, times r

def case(tier, name, what, commands, expected, method, source="analytic", base=None, sources=None):
    d = os.path.join(ROOT, tier, name)
    os.makedirs(d, exist_ok=True)
    recipe = {}
    if base:
        recipe["base"] = base
    recipe["commands"] = commands
    exp = {"sources": sources or [], "what": what, "source": source, "method": method}
    for k, v in expected.items():
        if isinstance(v, float):
            v = round(v, 9)
        exp[k] = v
    with open(os.path.join(d, "recipe.json"), "w") as f:
        json.dump(recipe, f, indent=1)
        f.write("\n")
    with open(os.path.join(d, "expected.json"), "w") as f:
        json.dump(exp, f, indent=1)
        f.write("\n")
    CASES.append((tier, name))

def missing(tier, name, what, why, method):
    d = os.path.join(ROOT, tier, name)
    os.makedirs(d, exist_ok=True)
    with open(os.path.join(d, "recipe.json"), "w") as f:
        json.dump({"commands": []}, f, indent=1)
        f.write("\n")
    with open(os.path.join(d, "expected.json"), "w") as f:
        json.dump({"sources": [], "what": what, "source": "invariants", "method": method, "missing": why}, f, indent=1)
        f.write("\n")
    CASES.append((tier, name))

def iso_material_fraction_ext(d, P):
    """ISO 68-1 basic external profile: material fraction m(r) per pitch, r in [r_root, d/2]."""
    H = math.sqrt(3) / 2 * P
    r1 = d / 2 - 5 * H / 8
    t = math.tan(PI / 6)
    return r1, (lambda r: 1.0 if r <= r1 else 1 - (P / 4 + 2 * (r - r1) * t) / P)

def iso_groove_fraction_int(d, P):
    """ISO 68-1 basic internal profile: fraction of a pitch the groove takes at radius r <= d/2."""
    t = math.tan(PI / 6)
    return lambda r: (P / 8 + 2 * (d / 2 - r) * t) / P

def cyclic_spline(P):
    """The periodic C2 cubic through the points P (2D, not repeated at the end), chord-length knots
    (crates/sketch/src/periodic.rs): per span (h, f) with f(s) -> ((x, x'), (y, y')), s in [0, h]."""
    n = len(P)
    h = [math.dist(P[i], P[(i + 1) % n]) for i in range(n)]
    def moments(y):
        # h[i-1] M[i-1] + 2 (h[i-1] + h[i]) M[i] + h[i] M[i+1] = 6 (slope after - slope before)
        A = [[0.0] * n for _ in range(n)]
        b = [0.0] * n
        for i in range(n):
            ip, im = (i + 1) % n, (i - 1) % n
            A[i][im] += h[im]
            A[i][i] += 2 * (h[im] + h[i])
            A[i][ip] += h[i]
            b[i] = 6 * ((y[ip] - y[i]) / h[i] - (y[i] - y[im]) / h[im])
        for c in range(n):
            for r in range(n):
                if r != c:
                    f = A[r][c] / A[c][c]
                    A[r] = [a - f * q for a, q in zip(A[r], A[c])]
                    b[r] -= f * b[c]
        return [b[i] / A[i][i] for i in range(n)]
    M = [moments([p[k] for p in P]) for k in range(2)]
    def span(i):
        j = (i + 1) % n
        def f(s):
            a, c = (h[i] - s) / h[i], s / h[i]
            return tuple((a * P[i][k] + c * P[j][k] + ((a ** 3 - a) * M[k][i] + (c ** 3 - c) * M[k][j]) * h[i] ** 2 / 6,
                          (P[j][k] - P[i][k]) / h[i] + (-(3 * a * a - 1) * M[k][i] + (3 * c * c - 1) * M[k][j]) * h[i] / 6) for k in range(2))
        return h[i], f
    return [span(i) for i in range(n)]

def clip_half(poly, a, b, c):
    """The part of a convex polygon [(x, y)] where a x + b y + c <= 0 (Sutherland-Hodgman)."""
    out = []
    n = len(poly)
    for i in range(n):
        p, q = poly[i], poly[(i + 1) % n]
        fp, fq = a * p[0] + b * p[1] + c, a * q[0] + b * q[1] + c
        if fp <= 0:
            out.append(p)
        if (fp < 0 < fq) or (fq < 0 < fp):
            s = fp / (fp - fq)
            out.append((p[0] + s * (q[0] - p[0]), p[1] + s * (q[1] - p[1])))
    return out

def moment_r(poly):
    """int int r dA over a polygon in the (r, z) half-plane: what a ring of that section holds, over 2 pi."""
    m = 0.0
    for i in range(len(poly)):
        (x0, y0), (x1, y1) = poly[i], poly[(i + 1) % len(poly)]
        m += (x0 + x1) * (x0 * y1 - x1 * y0)
    return abs(m) / 6

def sweep_removed(section, z0, lead, turns, rate, regions, panels_per_turn=400):
    """What a screw sweep removes from an axisymmetric solid: `section` [(r, dz)] (convex, about the
    axial position z0), carried `turns` turns rising `lead` a turn and moving `rate` mm a turn off
    the axis (a taper); the solid is the union of disjoint convex `regions`, each a list of
    half-planes (a, b, c): a r + b z + c <= 0. A point (rho, zeta) of the section at turn t is at
    r = rho + rate t, z = z0 + zeta + lead t, so the volume element is 2 pi r dA dt (the map's
    Jacobian), and V = 2 pi int_0^turns (int int_{section_t within the solid} r dA) dt, exact while
    successive turns don't overlap (the groove is narrower than the lead). Gauss-Legendre on
    panels: the integrand is piecewise polynomial in t."""
    def at(t):
        placed = [(r + rate * t, z0 + dz + lead * t) for r, dz in section]
        total = 0.0
        for hp in regions:
            p = placed
            for a, b, c in hp:
                p = clip_half(p, a, b, c)
                if len(p) < 3:
                    break
            if len(p) >= 3:
                total += moment_r(p)
        return total
    pieces = max(1, int(math.ceil(turns * panels_per_turn)))
    return 2 * PI * quad_split(at, 0.0, turns, pieces, 6)

def groove_section(P, flat, crest, flank_lo, flank_hi, r_root, r_far, internal=False):
    """A thread groove's axial section [(r, dz)] between r_root (its root) and r_far (past the crest,
    out of the material), its root flat centred on dz = 0: the tooth is `flat` wide at the crest
    radius and both walls are straight flanks (flank_lo toward -z, flank_hi toward +z, degrees
    from the radial)."""
    tl, th = math.tan(math.radians(flank_lo)), math.tan(math.radians(flank_hi))
    w_root = P - flat - abs(crest - r_root) * (tl + th)
    def ext(r):
        d = abs(r - r_root)
        return w_root / 2 + d * tl, w_root / 2 + d * th
    (l0, h0), (l1, h1) = ext(r_root), ext(r_far)
    return [(r_root, -l0), (r_far, -l1), (r_far, h1), (r_root, h0)]
