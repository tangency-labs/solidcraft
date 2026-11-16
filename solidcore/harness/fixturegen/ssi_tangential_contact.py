#!/usr/bin/env python3
"""Writes fixtures/surface.surface_p2/ssi_tangential_contact.json (issue #395, WR-75).

A reproduction corpus of tangential and high-order surface/surface contacts, each a
`gssi` case (`SmSurface::GlobalSurfaceIntersect` of two `SmBSplineSurface`s over their
natural domains, approximation tolerance 1e-6 as in the ssi corpus of #17). Every
surface is an exact NURBS of a closed form, so the contact is known exactly:

- rational cylinders, spheres and tori are the 9-point (or 5-point) rational circles
  swept or revolved (`w = 1/sqrt(2)` at the corners), the way the ssi corpus gives
  them; analytic surfaces would take the special-case intersectors and never reach the
  marcher this issue is about;
- polynomial patches (`z = x^2`, `x^3`, `x^4`, `x^2 + y^2`, `x^2 - y^2`,
  `(x^2 + y^2 - 1)^2`) are Bezier patches whose control nets are the exact Bernstein
  coefficients of the polynomial.

Each case's `upstream_ref` states the exact contact. Every case also runs with the
operands swapped (`-swapped`), as the intersector is not symmetric. The difftest gate
compares the pinned C++ with the default Rust kernel (parity); the opt-in tracer
(`Kernel::fixes.ssi_tangent_curve_tracer`) is checked against the closed forms in
`crates/solidcore/tests/issue_395_tangential_contact_corpus.rs`.

Run from the repo root: `python3 -I harness/fixturegen/ssi_tangential_contact.py`.
"""

import json
import math
import os
from fractions import Fraction

ROOT = os.getcwd()
OUT = os.path.join(ROOT, "fixtures/surface.surface_p2/ssi_tangential_contact.json")
TOL = 1.0e-6  # the ssi corpus tolerance (#17)
NOT_RATIONAL = -1.7976931348623157e308  # NOW: the control point carries no weight
W = math.sqrt(0.5)

# the 9-point unit circle (x, y, w) from +x, counter-clockwise, knots at quarters
CIRCLE = [
    (1, 0, 1), (1, 1, W), (0, 1, 1), (-1, 1, W), (-1, 0, 1),
    (-1, -1, W), (0, -1, 1), (1, -1, W), (1, 0, 1),
]
CIRCLE_KNOTS = [0, 0, 0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1, 1, 1]
# the 5-point half circle (radial, axial, w) from -axis to +axis through radial 1
HALF = [(0, -1, 1), (1, -1, W), (1, 0, 1), (1, 1, W), (0, 1, 1)]
HALF_KNOTS = [0, 0, 0, 0.5, 0.5, 1, 1, 1]


def add(a, b):
    return [a[0] + b[0], a[1] + b[1], a[2] + b[2]]


def mul(s, a):
    return [s * a[0], s * a[1], s * a[2]]


def hom(p, w):
    """The stored control point: `w x, w y, w z, w`."""
    return [p[0] * w, p[1] * w, p[2] * w, w]


def plain(p):
    return [float(p[0]), float(p[1]), float(p[2]), NOT_RATIONAL]


def srf(net, p, q, u, v):
    return {"net": net, "p": p, "q": q, "u": [float(x) for x in u], "v": [float(x) for x in v]}


def domain(s):
    return [s["u"][0], s["v"][0], s["u"][-1], s["v"][-1]]


def plane(x0, x1, y0, y1, z=0.0):
    """The bilinear patch of `z = const` over `[x0, x1] x [y0, y1]`."""
    net = [[plain([x, y, z]) for y in (y0, y1)] for x in (x0, x1)]
    return srf(net, 1, 1, [0, 0, 1, 1], [0, 0, 1, 1])


def cylinder(center, e1, e2, axis, r, h0, h1):
    """The cylinder `center + r (cos e1 + sin e2) + h axis`, `h` in `[h0, h1]`: u the
    circle (degree 2, rational), v the axis (degree 1)."""
    net = []
    for cx, cy, w in CIRCLE:
        base = add(center, add(mul(r * cx, e1), mul(r * cy, e2)))
        net.append([hom(add(base, mul(h, axis)), w) for h in (h0, h1)])
    return srf(net, 2, 1, CIRCLE_KNOTS, [0, 0, 1, 1])


def revolved(center, e1, e2, axis, profile, knots):
    """`profile` (`(radial, axial, w)` control points, degree 2) revolved about `axis`
    through `center`: u the full turn from `e1` towards `e2`, v the profile."""
    net = []
    for cx, cy, wu in CIRCLE:
        row = []
        for rho, z, wv in profile:
            p = add(center, add(mul(rho * cx, e1), add(mul(rho * cy, e2), mul(z, axis))))
            row.append(hom(p, wu * wv))
        net.append(row)
    return srf(net, 2, 2, CIRCLE_KNOTS, knots)


def sphere(center, e1, e2, axis, r):
    return revolved(center, e1, e2, axis, [(r * a, r * b, w) for a, b, w in HALF], HALF_KNOTS)


def torus(center, e1, e2, axis, big, small):
    prof = [(big + small * a, small * b, w) for a, b, w in CIRCLE]
    return revolved(center, e1, e2, axis, prof, CIRCLE_KNOTS)


def choose(n, k):
    return Fraction(math.comb(n, k))


def bernstein(poly, n, a, b):
    """The degree-`n` Bernstein coefficients over `t in [0, 1]` of `poly(x)`
    (`{power: coefficient}`), `x = a + (b - a) t`, exactly."""
    # monomial coefficients in t
    mono = [Fraction(0)] * (n + 1)
    for k, c in poly.items():
        # (a + d t)^k
        d = Fraction(b) - Fraction(a)
        for j in range(k + 1):
            mono[j] += Fraction(c) * choose(k, j) * Fraction(a) ** (k - j) * d**j
    return [sum(choose(i, j) / choose(n, j) * mono[j] for j in range(i + 1)) for i in range(n + 1)]


def bezier_graph(terms, n, lo, hi):
    """The graph `z = sum c x^i y^j` (`terms`: `(i, j, c)`) over `[lo, hi]^2` as a
    degree-`(n, n)` Bezier patch (exact control net)."""
    bx = {k: bernstein({k: 1}, n, lo, hi) for k in range(n + 1)}
    xs = bernstein({1: 1}, n, lo, hi)
    net = []
    for i in range(n + 1):
        row = []
        for j in range(n + 1):
            z = sum(Fraction(c) * bx[a][i] * bx[e][j] for a, e, c in terms)
            row.append(plain([float(xs[i]), float(xs[j]), float(z)]))
        net.append(row)
    knots = [0] * (n + 1) + [1] * (n + 1)
    return srf(net, n, n, knots, knots)


def ruled_graph(power, n, lo, hi, y0, y1):
    """The ruled graph `z = x^power` over `[lo, hi] x [y0, y1]`: degree `n` in u (x),
    1 in v (y)."""
    xs = bernstein({1: 1}, n, lo, hi)
    zs = bernstein({power: 1}, n, lo, hi)
    net = [[plain([float(xs[i]), y, float(zs[i])]) for y in (y0, y1)] for i in range(n + 1)]
    return srf(net, n, 1, [0] * (n + 1) + [1] * (n + 1), [0, 0, 1, 1])


X, Y, Z = [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]
O = [0.0, 0.0, 0.0]

PAIRS = [
    (
        "cylinder_plane_line",
        "a unit cylinder (axis y through (0, 0, 1), y in [-1, 1]) on the plane z = 0: "
        "tangent along the segment (0, -1..1, 0), length 2",
        cylinder([0, -1, 1], X, Z, Y, 1.0, 0.0, 2.0),
        plane(-2, 2, -2, 2),
    ),
    (
        "parabolic_cylinder_plane_line",
        "z = x^2 over [-1, 1]^2 on the plane z = 0: tangent (2nd order) along the segment "
        "(0, -1..1, 0), length 2",
        ruled_graph(2, 2, -1, 1, -1.0, 1.0),
        plane(-2, 2, -2, 2),
    ),
    (
        "cylinders_parallel_external_line",
        "two unit cylinders with parallel axes 2 apart (axis y through (0, 0, 1), "
        "y in [-1, 1], and through (0, 0, -1), y in [-2, 2]): tangent along the segment "
        "(0, -1..1, 0), length 2",
        cylinder([0, -1, 1], X, Z, Y, 1.0, 0.0, 2.0),
        cylinder([0, -2, -1], X, Z, Y, 1.0, 0.0, 4.0),
    ),
    (
        "cylinders_parallel_internal_line",
        "a unit cylinder (axis y through (0, 0, -1), y in [-1, 1]) inside a radius-2 "
        "cylinder (axis y through the origin, y in [-2, 2]): tangent along the segment "
        "(0, -1..1, -2), length 2",
        cylinder([0, -1, -1], X, Z, Y, 1.0, 0.0, 2.0),
        cylinder([0, -2, 0], X, Z, Y, 2.0, 0.0, 4.0),
    ),
    (
        "steinmetz_equal_cylinders",
        "two unit cylinders with crossing perpendicular axes (z and x through the origin, "
        "length 4): two ellipses in x = +-z that cross at the tangent contact nodes "
        "(0, +-1, 0)",
        cylinder([0, 0, -2], X, Y, Z, 1.0, 0.0, 4.0),
        cylinder([-2, 0, 0], Y, Z, X, 1.0, 0.0, 4.0),
    ),
    (
        "sphere_plane_point",
        "a unit sphere (centre (0, 0, 1), poles on the x axis) on the plane z = 0: one "
        "contact point (0, 0, 0), off the poles",
        sphere([0, 0, 1], Y, Z, X, 1.0),
        plane(-2, 2, -2, 2),
    ),
    (
        "paraboloid_plane_point",
        "z = x^2 + y^2 over [-1, 1]^2 on the plane z = 0: one contact point (0, 0, 0)",
        bezier_graph([(2, 0, 1), (0, 2, 1)], 2, -1, 1),
        plane(-2, 2, -2, 2),
    ),
    (
        "torus_plane_circle",
        "a torus (axis z, R 2, r 0.5, centre (0, 0, 0.5)) on the plane z = 0: tangent "
        "along the circle x^2 + y^2 = 4, z = 0 (closed, length 4 pi)",
        torus([0, 0, 0.5], X, Y, Z, 2.0, 0.5),
        plane(-3, 3, -3, 3),
    ),
    (
        "quartic_plane_circle",
        "z = (x^2 + y^2 - 1)^2 over [-2, 2]^2 on the plane z = 0 (ssi corpus "
        "c2n_quartic_circle): tangent along the unit circle (closed, length 2 pi)",
        bezier_graph(
            [(4, 0, 1), (0, 4, 1), (2, 2, 2), (2, 0, -2), (0, 2, -2), (0, 0, 1)], 4, -2, 2
        ),
        plane(-3, 3, -3, 3),
    ),
    (
        "sphere_in_cylinder_circle",
        "a unit sphere (centre the origin, poles on the x axis) in a unit cylinder (axis x, "
        "x in [-2, 2]): tangent along the circle x = 0, y^2 + z^2 = 1 (closed, length 2 pi)",
        sphere(O, Y, Z, X, 1.0),
        cylinder([-2, 0, 0], Y, Z, X, 1.0, 0.0, 4.0),
    ),
    (
        "cubic_plane_line",
        "z = x^3 over [-1, 1]^2 on the plane z = 0: a crossing (odd, 3rd order) contact "
        "along the segment (0, -1..1, 0), length 2",
        ruled_graph(3, 3, -1, 1, -1.0, 1.0),
        plane(-2, 2, -2, 2),
    ),
    (
        "quartic_ridge_plane_line",
        "z = x^4 over [-1, 1]^2 on the plane z = 0: a touching (4th order) contact along "
        "the segment (0, -1..1, 0), length 2",
        ruled_graph(4, 4, -1, 1, -1.0, 1.0),
        plane(-2, 2, -2, 2),
    ),
    (
        "saddle_plane_node",
        "z = x^2 - y^2 over [-1, 1]^2 on the plane z = 0: the diagonals y = +-x (each "
        "length 2 sqrt 2) crossing at the tangent contact node (0, 0, 0)",
        bezier_graph([(2, 0, 1), (0, 2, -1)], 2, -1, 1),
        plane(-2, 2, -2, 2),
    ),
    (
        "parabolic_cylinders_crossing_node",
        "z = x^2 and z = y^2 over [-1, 1]^2: the curves y = +-x, z = x^2 crossing at the "
        "tangent contact node (0, 0, 0)",
        bezier_graph([(2, 0, 1)], 2, -1, 1),
        bezier_graph([(0, 2, 1)], 2, -1, 1),
    ),
]


def case(name, ref, a, b):
    return {
        "name": name,
        "upstream_ref": ref,
        "inputs": {
            "this": {"srf": a},
            "op": "gssi",
            "dom": domain(a),
            "other": {"srf": b},
            "other_dom": domain(b),
            "tol": TOL,
        },
    }


def main():
    cases = []
    for name, what, a, b in PAIRS:
        ref = f"issue #395 (WR-75) tangential contact corpus: {what}"
        cases.append(case(f"tc_{name}", ref, a, b))
        cases.append(case(f"tc_{name}-swapped", ref + "; operands swapped", b, a))
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump({"cases": cases}, f, indent=1)
        f.write("\n")
    print(f"{len(cases)} cases -> {os.path.relpath(OUT, ROOT)}")


if __name__ == "__main__":
    main()
