#!/usr/bin/env python3
"""Closed-form expected values for the fillets whose radius varies along a straight edge
(cases 77-81 of tests/oracle: variable, closing at a corner, chord length, taper to fit, a
B-spline radius).

Standalone, Python 3 stdlib only (it reuses the helpers of cases_features.py). Run from the
worktree root:

    python3 tools/oracle-analytic/cases_fillets.py

Each case rounds one straight edge where two faces meet at a right angle: the edge along x at
y = 0, z = H, between the front face (y = 0) and the top face (z = H). Every section square to
the edge is a quarter-circle round of radius r(x), so with the spandrel of radius r (the r x r
corner minus the quarter disc, area (1 - pi/4) r^2, centroid r (10 - 3 pi) / (3 (4 - pi)) from
both faces):

    V removed = int (1 - pi/4) r^2 dx
    moments   = int (x, off(r), H - off(r)) (1 - pi/4) r^2 dx
    area      = - 2 int r dx               (a strip r wide off each face)
                - (1 - pi/4) (r(0)^2 + r(L)^2)  (the spandrel off each end face)
                + the round's own area, int int |P_x x P_t| dt dx over
                  P(x, t) = (x, r (1 - cos t), H - r (1 - sin t)), t in [0, pi/2]

r is a polynomial on each piece of the edge, so the volume and moments are integrated exactly
by Gauss-Legendre per piece; the round's area (a square root) to round-off with 24 x 24 points.

That quarter round is the round only where r is constant (case 79). Where r varies the round is
the union of the balls rolled along the edge (`removed_by_balls`): the ball at s sits r(s) off both
faces, its centre (s, r, H - r), the law carried on past the edge's ends along its end slopes. In
the plane x, the ray from the edge at the angle t (t = 0 along the top face, pi/2 down the front
face: direction (0, cos t, -sin t)) first meets the union at rho(x, t), the least over s of its entry
into ball s, p - sqrt(p^2 - (x - s)^2 - r^2) with p = r (cos t + sin t). The removed region is
q < rho in polar coordinates about the edge:
    V = int int rho^2 / 2 dt dx,  moments int int (x rho^2 / 2, rho^3 cos t / 3, H rho^2 / 2 - rho^3 sin t / 3)
    area: the faces lose rho(x, 0) and rho(x, pi/2) wide strips and each end face its slice, the
          round adds int int |P_x x P_t| over P(x, t) = (x, rho cos t, H - rho sin t).
t runs through t = pi/4 (1 - cos pi q) (rho is tangent to the faces at both ends), each integral by
Gauss-Legendre on PANELS panels of x and q per piece. With 2, 4 and 8 panels the values agree to
1e-9 of the body's: rho is smooth in each piece save along the curves where its nearest ball crosses
a break of the law (a knot, or an end where the law goes on along its slope), which bound the area's
convergence.
"""

import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from cases_features import GL_W, GL_X, MP, PI, box, extrude, extrude_area, poly, region, spandrel_area, spandrel_offset, write  # noqa: E402

K = 1 - PI / 4
PANELS = 8


def removed(radius, slope, pieces, H):
    """What rounding the edge (x from 0 to L, corner at y = 0, z = H) with radius(x) (and its
    slope(x)) takes away: its mass properties and the change of area. `pieces` are the x intervals
    on which r is one polynomial."""
    v = mx = my = mz = strips = blend = 0.0
    for a, b in pieces:
        h = (b - a) / 2
        for xg, wg in zip(GL_X, GL_W):
            x = (a + b) / 2 + h * xg
            w = wg * h
            r, s = radius(x), slope(x)
            sec = spandrel_area(r)
            v += w * sec
            mx += w * x * sec
            my += w * spandrel_offset(r) * sec
            mz += w * (H - spandrel_offset(r)) * sec
            strips += w * 2 * r
            # The round: |P_x x P_t| with P_x = (1, s (1 - cos t), -s (1 - sin t)),
            # P_t = (0, r sin t, r cos t).
            for tg, tw in zip(GL_X, GL_W):
                t = PI / 4 * (1 + tg)
                px = (1.0, s * (1 - math.cos(t)), -s * (1 - math.sin(t)))
                pt = (0.0, r * math.sin(t), r * math.cos(t))
                c = (px[1] * pt[2] - px[2] * pt[1], px[2] * pt[0] - px[0] * pt[2], px[0] * pt[1] - px[1] * pt[0])
                blend += w * tw * PI / 4 * math.sqrt(sum(q * q for q in c))
    ends = spandrel_area(radius(pieces[0][0])) + spandrel_area(radius(pieces[-1][1]))
    return MP(v, [mx, my, mz]), blend - strips - ends


def rho_of(radius, slope, length):
    """rho(x, t): where the ray from the edge at the angle t first meets the balls' union (the law
    carried on past [0, length] along its end slopes)."""
    r0, r1, s0, s1 = radius(0.0), radius(length), slope(0.0), slope(length)
    big = max(max(radius(length * i / 200) for i in range(201)), 1e-9)

    def r(s):
        return r0 + s0 * s if s < 0 else (r1 + s1 * (s - length) if s > length else radius(s))

    def entry(x, c, sn, s):
        rr = r(s)
        if rr <= 0:
            return math.inf
        p = rr * (c + sn)
        d = p * p - (x - s) ** 2 - rr * rr
        return p - math.sqrt(d) if d >= 0 else math.inf

    def reach(x, w, s):
        # > 0 where the ray at t meets ball s: p^2 - (x - s)^2 - r^2 = r^2 sin 2t - (x - s)^2,
        # w = sqrt(sin 2t)
        return w * r(s) - abs(x - s)

    def end(x, w, inside, outside):
        for _ in range(100):
            m = 0.5 * (inside + outside)
            if reach(x, w, m) > 0:
                inside = m
            else:
                outside = m
        return inside

    def rho(x, t):
        rx = r(x)
        if rx <= 0:
            # no ball reaches the plane x (the law has closed to 0): the section keeps its corner
            return 0.0
        if t <= 0.0 or t >= PI / 2:
            # along a face the ray is tangent to every ball and touches only ball x
            return rx
        c, sn = math.cos(t), math.sin(t)
        w = math.sqrt(2 * sn * c)
        n = 240
        # The balls the ray meets around ball x span [sl, sr], r sqrt(sin 2t) either side: near the
        # faces or where r is small that is narrow beside the global scan's step, so it is scanned
        # on its own.
        lo, hi = x - 1.5 * big, x + 1.5 * big
        step = (hi - lo) / n
        sl = x
        while sl > lo and reach(x, w, sl - step) > 0:
            sl -= step
        sl = end(x, w, sl, sl - step)
        sr = x
        while sr < hi and reach(x, w, sr + step) > 0:
            sr += step
        sr = end(x, w, sr, sr + step)
        best, bs, h = math.inf, x, step
        for a, b in ((lo, hi), (sl, sr)):
            for i in range(n + 1):
                s = a + (b - a) * i / n
                e = entry(x, c, sn, s)
                if e < best:
                    best, bs, h = e, s, (b - a) / n
        a, b = bs - h, bs + h
        g = (math.sqrt(5) - 1) / 2
        for _ in range(90):
            m1, m2 = b - g * (b - a), a + g * (b - a)
            if entry(x, c, sn, m1) < entry(x, c, sn, m2):
                b = m2
            else:
                a = m1
        return min(best, entry(x, c, sn, 0.5 * (a + b)))
    return rho


def removed_by_balls(radius, slope, pieces, H, panels=4):
    """As `removed`, for the union of the balls (the module's notes): mass properties of what is
    taken and the change of area, over `panels` Gauss-Legendre panels per piece and across."""
    length = pieces[-1][1]
    rho = rho_of(radius, slope, length)
    v = mx = my = mz = strips = blend = 0.0

    def slice_area(x):
        a = 0.0
        for k in range(panels):
            for qg, qw in zip(GL_X, GL_W):
                q = (k + (1 + qg) / 2) / panels
                t = PI / 4 * (1 - math.cos(PI * q))
                a += qw / (2 * panels) * PI * PI / 4 * math.sin(PI * q) * rho(x, t) ** 2 / 2
        return a
    for a0, b0 in pieces:
        for kx in range(panels):
            a, b = a0 + (b0 - a0) * kx / panels, a0 + (b0 - a0) * (kx + 1) / panels
            hx = (b - a) / 2
            for xg, wg in zip(GL_X, GL_W):
                x = (a + b) / 2 + hx * xg
                wx = wg * hx
                strips += wx * (rho(x, 0.0) + rho(x, PI / 2))
                dx = 1e-5 * max(1.0, length)
                for k in range(panels):
                    for qg, qw in zip(GL_X, GL_W):
                        q = (k + (1 + qg) / 2) / panels
                        t = PI / 4 * (1 - math.cos(PI * q))
                        dt = PI * PI / 4 * math.sin(PI * q)  # dt / dq
                        w = wx * qw / (2 * panels) * dt
                        p = rho(x, t)
                        c, sn = math.cos(t), math.sin(t)
                        v += w * p * p / 2
                        mx += w * x * p * p / 2
                        my += w * p ** 3 * c / 3
                        mz += w * (H * p * p / 2 - p ** 3 * sn / 3)
                        # P(x, q) = (x, p cos t, H - p sin t): rho meets the faces with a square-root
                        # slope in t, smooth in q, so its derivative is taken in q
                        e = 1e-6
                        qa, qb = max(q - e, 0.0), min(q + e, 1.0)
                        ta, tb = PI / 4 * (1 - math.cos(PI * qa)), PI / 4 * (1 - math.cos(PI * qb))
                        px_ = (rho(min(x + dx, b0), t) - rho(max(x - dx, a0), t)) / (min(x + dx, b0) - max(x - dx, a0))
                        pq_ = (rho(x, tb) - rho(x, ta)) / (qb - qa)
                        Px = (1.0, px_ * c, -px_ * sn)
                        Pq = (0.0, pq_ * c - p * sn * dt, -pq_ * sn - p * c * dt)
                        cr = (Px[1] * Pq[2] - Px[2] * Pq[1], Px[2] * Pq[0] - Px[0] * Pq[2], Px[0] * Pq[1] - Px[1] * Pq[0])
                        blend += wx * qw / (2 * panels) * math.sqrt(sum(z * z for z in cr))
    ends = slice_area(pieces[0][0]) + slice_area(pieces[-1][1])
    return MP(v, [mx, my, mz]), blend - strips - ends


def linear(a, b, length):
    """r running straight from a (x = 0) to b (x = length)."""
    return (lambda x: a + (b - a) * x / length), (lambda x: (b - a) / length)


# What a variable round's value is, for the method strings
BALLS = ("The round is the union of the balls rolled along the edge, ball s touching both faces at r(s), the law "
         "continued past the edge's ends along its end slopes: each section loses the corner points no ball reaches, "
         "up to rho(x, t), the nearest entry into a ball along the ray from the edge at the angle t. V, moments and "
         "areas integrated by Gauss-Legendre panels in x and t")


def c77():
    r, dr = linear(2.0, 5.0, 40.0)
    cut, da = removed_by_balls(r, dr, [(0.0, 40.0)], 20.0, PANELS)
    write("77-fillet-variable-linear",
          "box 40x30x20 with its top front edge (y=0, z=20) rounded at a radius running straight from 2 (x=0) to 5 (x=40). "
          + BALLS + " (tools/oracle-analytic/cases_fillets.py)",
          1, box(0, 0, 0, 40, 30, 20) - cut, 5200 + da, [(0, 0, 0), (40, 30, 20)], faces=7)


def c78():
    r, dr = linear(4.0, 0.0, 40.0)
    cut, da = removed_by_balls(r, dr, [(0.0, 40.0)], 20.0, PANELS)
    write("78-fillet-variable-to-zero",
          "as 77 with the radius running from 4 (x=0) down to 0 (x=40), where the round closes to the corner: the "
          "round is three-sided and the corner at x=40 stays (no ball reaches that section). " + BALLS,
          1, box(0, 0, 0, 40, 30, 20) - cut, 5200 + da, [(0, 0, 0), (40, 30, 20)], faces=7)


def c79():
    rc = 4.0 / (2 * math.cos(PI / 4))
    cut, da = removed(lambda x: rc, lambda x: 0.0, [(0.0, 40.0)], 20.0)
    write("79-fillet-chord-length",
          "box 40x30x20, top front edge rounded 4 wide across (chord c): r = c / (2 cos 45 deg) = 2 sqrt 2; "
          "V = abc - (1 - pi/4) r^2 L, A = 2(ab + bc + ca) - 2 r L + (pi/2) r L - 2 (1 - pi/4) r^2",
          1, box(0, 0, 0, 40, 30, 20) - cut, 5200 + da, [(0, 0, 0), (40, 30, 20)], faces=7)


def bspline(degree, knots, ctrl, length):
    """A clamped B-spline radius in u = x / length (the kernel's `RadiusSpline`): r(x), its slope
    dr/dx (de Boor on the span holding u, the slope from the differenced controls) and the x
    pieces between distinct knots, on which r is one polynomial."""
    n = len(ctrl)

    def deboor(p, kn, c, u):
        k = max(i for i in range(p, len(c)) if kn[i] <= u) if u < kn[len(c)] else len(c) - 1
        d = [c[j + k - p] for j in range(p + 1)]
        for rr in range(1, p + 1):
            for j in range(p, rr - 1, -1):
                i = j + k - p
                al = (u - kn[i]) / (kn[i + p + 1 - rr] - kn[i])
                d[j] = (1 - al) * d[j - 1] + al * d[j]
        return d[p]

    dctrl = [degree * (ctrl[i + 1] - ctrl[i]) / (knots[i + degree + 1] - knots[i + 1]) for i in range(n - 1)]
    breaks = sorted(set(knots))
    pieces = [(length * a, length * b) for a, b in zip(breaks, breaks[1:])]
    return (lambda x: deboor(degree, knots, ctrl, x / length)), (lambda x: deboor(degree - 1, knots[1:-1], dctrl, x / length) / length), pieces


# The taper law (crates/kernel/src/blend_variable.rs `taper`): a clamped cubic B-spline in
# u = x / L on 8 equal spans, its control radii min(R, 0.9 room) at the Greville abscissae.
SPANS, SHARE = 8, 0.9


def taper(radius, room, length):
    knots = [0.0] * 4 + [k / SPANS for k in range(1, SPANS)] + [1.0] * 4
    greville = [sum(knots[j + 1:j + 4]) / 3 for j in range(SPANS + 3)]
    ctrl = [min(radius, SHARE * room(u)) for u in greville]
    return (*bspline(3, knots, ctrl, length), ctrl)


def c80():
    # Plate: top face narrowing from 20 (x=0) to 2 (x=40), 15 high. The room for the round at u:
    # the top face's width and the front face's height, times tan 45 deg.
    r, dr, pieces, ctrl = taper(3.0, lambda u: min(20 - 18 * u, 15.0), 40.0)
    reg = region([poly([(0, 0), (40, 0), (40, 2), (0, 20)])])
    cut, da = removed_by_balls(r, dr, pieces, 15.0, PANELS)
    write("80-fillet-taper-to-fit",
          "plate (top 20 -> 2 wide over x 0..40, 15 high) with its top front edge filleted at 3, fit: taper. The radius "
          "is the taper law: a clamped cubic B-spline in u = x/40 on 8 equal spans, control radii min(3, 0.9 room) at the "
          "Greville abscissae, room = min(20 - 18u, 15) tan 45 deg (top face width, front face height): "
          + ", ".join(f"{c:.4f}" for c in ctrl) + ". " + BALLS + ", per knot span",
          1, extrude(reg, 0, 15) - cut, extrude_area(reg, 15) + da, [(0, 0, 0), (40, 20, 15)], faces=7)


def c81():
    knots = [0.0, 0.0, 0.0, 0.0, 0.25, 0.6, 1.0, 1.0, 1.0, 1.0]
    ctrl = [2.0, 5.0, 1.5, 4.0, 1.0, 3.0]
    r, dr, pieces = bspline(3, knots, ctrl, 40.0)
    cut, da = removed_by_balls(r, dr, pieces, 20.0, PANELS)
    write("81-fillet-spline-radius",
          "box 40x30x20 with its top front edge (y=0, z=20) rounded at a radius that is a clamped cubic B-spline in "
          "u = x/40, knots 0,0,0,0,0.25,0.6,1,1,1,1, control radii 2, 5, 1.5, 4, 1, 3 (r(0) = 2, r(40) = 3). "
          + BALLS + ", per knot span",
          1, box(0, 0, 0, 40, 30, 20) - cut, 5200 + da, [(0, 0, 0), (40, 30, 20)], faces=7)


CASES = [c77, c78, c79, c80, c81]

if __name__ == "__main__":
    for fn in CASES:
        fn()
