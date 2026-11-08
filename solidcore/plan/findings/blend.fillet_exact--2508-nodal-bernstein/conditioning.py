"""Numerical diagnosis of the existing nodal-quartic formula, not kernel code.

Replicates rational.rs's power coefficients for its two-cylinder unit-test input.
The Decimal control evaluates that same formula to 70 digits from the same endpoints.
"""
import csv
import decimal
import json
import math
from pathlib import Path

decimal.getcontext().prec = 70
D = decimal.Decimal
OUT = Path(__file__).resolve().parent
OUT.mkdir(exist_ok=True)

def coefficients(number, sqrt):
    """Form the old quartic's homogeneous power coefficients in the given arithmetic.

    The endpoints are shared double inputs; using Decimal isolates coefficient
    formation error from the later evaluation of those rounded coefficients.
    """
    def dot(a, b):
        """Dot product in the selected arithmetic, preserving the original sum order."""
        s = number(0)
        for x, y in zip(a, b):
            s += x * y
        return s
    def bil(a, u, v):
        """Evaluate uᵀAv in the original row order without a basis conversion."""
        s = number(0)
        for x, row in zip(u, a):
            for m, y in zip(row, v):
                s += x * m * y
        return s
    n = number
    a1 = [[n(1), n(0), n(0)], [n(0), n(1), n(0)], [n(0), n(0), n(0)]]
    a2 = [[n(0), n(0), n(0)], [n(0), n(1), n(0)], [n(0), n(0), n(1)]]
    q = [n(0), n('3.5'), n(0)]
    g1 = q
    g2 = [n(0), n('1.5'), n(0)]
    k = dot(g1, g2) / dot(g1, g1)
    c = [[v - k * u for u, v in zip(r1, r2)] for r1, r2 in zip(a1, a2)]
    root = sqrt(-c[0][0] * c[2][2])
    v0 = [c[2][2], n(0), -root]
    length = sqrt(dot(v0, v0))
    v0 = [v / length for v in v0]
    def on(phi):
        """The positive-x lobe on both offset cylinders at the given angle."""
        y = 2.0 + 1.5 * math.cos(phi)
        return [math.sqrt(3.5 * 3.5 - y * y), y, 1.5 * math.sin(phi)]
    f = [n(x) - qi for x, qi in zip(on(0.3), q)]
    t = [n(x) - qi for x, qi in zip(on(2 * math.pi - 0.3), q)]
    w0 = f
    w1 = [v - u for u, v in zip(f, t)]
    qc = [bil(c, w0, w0), n(2) * bil(c, w0, w1), bil(c, w1, w1)]
    be0, be1 = bil(c, v0, w0), bil(c, v0, w1)
    vc = [[qc[0] * v - n(2) * be0 * u for v, u in zip(v0, w0)],
          [qc[1] * v - n(2) * (be0 * z + be1 * u) for v, u, z in zip(v0, w0, w1)],
          [qc[2] * v - n(2) * be1 * z for v, z in zip(v0, w1)]]
    g = [dot(g1, v) for v in vc]
    w = [n(0)] * 5
    for i, u in enumerate(vc):
        for j, v in enumerate(vc):
            w[i + j] += bil(a1, u, v)
    x = [[qi * wi for wi in w] for qi in q]
    for row, coord in zip(x, range(3)):
        for i, gi in enumerate(g):
            for j, v in enumerate(vc):
                row[i + j] -= n(2) * gi * v[coord]
    return x, w

def horner(p, t):
    """Evaluate an ascending power-coefficient sequence by Horner's rule."""
    s = p[0] * 0
    for c in reversed(p):
        s = s * t + c
    return s

xf, wf = coefficients(float, math.sqrt)
xd, wd = coefficients(D, lambda x: x.sqrt())
rounded_xd = [[D(x) for x in row] for row in xf]
rounded_wd = [D(w) for w in wf]
rounded_worst = [0.0, 0.0]
rows = []
for i in range(1001):
    t = i / 1000
    w = horner(wf, t)
    p = [horner(x, t) / w for x in xf]
    dt = D(i) / D(1000)
    exact_w = horner(wd, dt)
    ref = [float(horner(x, dt) / exact_w) for x in xd]
    rounded_w = horner(rounded_wd, dt)
    rounded_p = [horner(x, dt) / rounded_w for x in rounded_xd]
    rounded_worst[0] = max(rounded_worst[0], float(abs((rounded_p[0]**2 + rounded_p[1]**2).sqrt() - D('3.5'))))
    rounded_worst[1] = max(rounded_worst[1], float(abs(((rounded_p[1] - D(2))**2 + rounded_p[2]**2).sqrt() - D('1.5'))))
    off0 = abs(math.sqrt(p[0]**2 + p[1]**2) - 3.5)
    off1 = abs(math.sqrt((p[1] - 2)**2 + p[2]**2) - 1.5)
    cancellation = sum(abs(c * t**i) for i, c in enumerate(wf)) / abs(w)
    rows.append([t, *p, *ref, off0, off1, cancellation])
with (OUT / 'power-basis.csv').open('w') as f:
    writer = csv.writer(f)
    writer.writerow(['t', 'x', 'y', 'z', 'ref_x', 'ref_y', 'ref_z', 'offset_0', 'offset_1', 'cancellation'])
    writer.writerows(rows)
print(json.dumps({'max_offset_0': max(r[7] for r in rows),
                  'max_offset_1': max(r[8] for r in rows),
                  'max_cancellation': max(r[9] for r in rows),
                  'rounded_power_coefficients_evaluated_at_70_digits': rounded_worst}))

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
plt.rcParams.update({'font.family': 'sans-serif', 'font.size': 11})
fig = plt.figure(figsize=(12, 5.3), layout='constrained')
ax = fig.add_subplot(1, 2, 1, projection='3d')
ax.plot([r[4] for r in rows], [r[5] for r in rows], [r[6] for r in rows], color='#1f6fb2', linewidth=2.5)
ax.scatter([0], [3.5], [0], c='#555555', s=25)
ax.text(0, 3.5, 0, '  node')
ax.set(xlabel='x (mm)', ylabel='y (mm)', zlabel='z (mm)', title='Expected: spine on both offset cylinders')
ax.set_box_aspect([1, 1, 1])
ax.view_init(elev=25, azim=-50)
ax = fig.add_subplot(1, 2, 2)
ax.semilogy([r[0] for r in rows], [max(r[7], 1e-16) for r in rows], label='R = 3.5 offset', color='#1f6fb2')
ax.semilogy([r[0] for r in rows], [max(r[8], 1e-16) for r in rows], label='R = 1.5 offset', color='#555555')
ax.set(xlabel='arc parameter', ylabel='distance from offset (mm)', title='Existing formula: cancellation near the last end')
ax.grid(alpha=0.2)
ax.legend()
fig.suptitle('Issue 2508 — closed-form rational spine, power-basis evaluation', fontsize=16)
fig.savefig(OUT / 'power-basis.png', dpi=160)
