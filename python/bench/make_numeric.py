"""Writes tasks/numeric.json: tasks whose numbers need real computation (a polygon from a formula,
a least-squares fit, a table, an optimum, a convex hull, a revolved profile, a sizing rule).

Each task is built here with the package, and its expected bodies are what SolidCraft measures.
Where a closed form exists, the build is checked against it first, so a wrong reference build
stops this script instead of becoming the answer. Written before any model run; never tuned on.

    SOLIDCRAFT_CLI=... python bench/make_numeric.py
"""

from __future__ import annotations

import json
import math
from pathlib import Path

import numpy as np
from scipy.optimize import least_squares
from scipy.spatial import ConvexHull

import solidcraft as sc

HERE = Path(__file__).parent
TASKS: dict[str, dict] = {}


def bodies(d: sc.Session) -> list[dict]:
    m = d.measure(summary=True)
    return [{"name": b["name"], "volume": round(b["volume_mm3"], 4), "bbox": [b["bbox"]["min"], b["bbox"]["max"]]} for b in m["bodies"]]


def task(name: str, spec: str, build, volume: float | None = None) -> None:
    with sc.headless() as d:
        build(d)
        got = bodies(d)
    if volume is not None:
        total = sum(b["volume"] for b in got)
        assert abs(total - volume) <= 1e-6 * max(1.0, volume), f"{name}: built {total}, formula {volume}"
    TASKS[name] = {"spec": spec, "bodies": got}
    print(f"{name}: {len(got)} bodies, total volume {sum(b['volume'] for b in got):.4f}")


def polygon_area(pts: np.ndarray) -> float:
    x, y = pts[:, 0], pts[:, 1]
    return 0.5 * abs(np.dot(x, np.roll(y, -1)) - np.dot(y, np.roll(x, -1)))


def extrude_polygon(d: sc.Session, pts, height: float, name: str, z0: float = 0.0) -> str:
    sk = d.cmd.sketch.create(plane={"origin": [0, 0, z0], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]})["sketch"]
    d.cmd.sketch.line(points=[[float(x), float(y)] for x, y in pts], closed=True)
    d.cmd.sketch.finish()
    d.cmd.solid.extrude(sketch=sk, distance=height, body_name=name)
    return name


# 1. A cam outline from a formula.
th = np.radians(np.arange(0, 360, 5))
cam = np.column_stack([(40 + 8 * np.sin(3 * th)) * np.cos(th), (40 + 8 * np.sin(3 * th)) * np.sin(th)])


def build_cam(d):
    extrude_polygon(d, cam, 10, "cam")
    d.cmd.solid.cylinder(height=10, diameter=12, base=[0, 0, 0], operation="cut", targets=["cam"])


task(
    "cam",
    "A flat cam, one solid, 10 mm thick (z 0..10). Its outline is the closed polygon (straight lines) through the 72 points at "
    "angles θ = 0°, 5°, 10°, ..., 355° (measured from +X towards +Y), each at distance r(θ) = 40 + 8·sin(3θ) mm from the origin. "
    "A 12 mm diameter bore goes through it along Z, centred on the origin.",
    build_cam,
    volume=10 * polygon_area(cam) - math.pi * 36 * 10,
)

# 2. A least-squares circle through measured points.
rng = np.random.default_rng(7)
ang = np.radians(np.sort(rng.uniform(0, 360, 10)))
cx, cy, r0 = 12.3, -4.7, 23.45
meas = np.round(np.column_stack([cx + r0 * np.cos(ang), cy + r0 * np.sin(ang)]) + rng.normal(0, 0.04, (10, 2)), 3)
fit = least_squares(lambda p: np.hypot(meas[:, 0] - p[0], meas[:, 1] - p[1]) - p[2], [0, 0, 10]).x
fx, fy, fr = (round(float(v), 2) for v in fit)


def build_fit(d):
    d.cmd.solid.cylinder(height=15, diameter=2 * fr, base=[fx, fy, 0], body_name="boss")


task(
    "bestfit",
    "These 10 points (x, y in mm) were measured on the round edge of a boss: "
    + ", ".join(f"({x:g}, {y:g})" for x, y in meas)
    + ". Make the boss: one cylinder along Z from z 0 to 15 whose circle is the best fit through the points: the centre (cx, cy) "
    "and radius r that minimise the sum of squared distances from each point to the circle (geometric least squares). "
    "Round cx, cy and r to 0.01 mm before building.",
    build_fit,
    volume=math.pi * fr * fr * 15,
)

# 3. Standoffs from a table.
table = [
    ("S1", 10.0, 10.0, 12.0, 6.0),
    ("S2", 110.0, 10.0, 12.0, 6.0),
    ("S3", 10.0, 70.0, 12.0, 6.0),
    ("S4", 110.0, 70.0, 12.0, 6.0),
    ("S5", 45.5, 22.25, 8.5, 5.0),
    ("S6", 74.5, 22.25, 8.5, 5.0),
    ("S7", 45.5, 57.75, 8.5, 5.0),
    ("S8", 74.5, 57.75, 20.0, 7.5),
]
csv = "name,x,y,height,diameter\n" + "\n".join(f"{n},{x:g},{y:g},{h:g},{dd:g}" for n, x, y, h, dd in table)


def build_table(d):
    d.cmd.solid.box(length=120, width=80, height=3, corner=[0, 0, -3], body_name="base")
    for n, x, y, h, dd in table:
        d.cmd.solid.cylinder(height=h, diameter=dd, base=[x, y, 0], body_name=n)


task(
    "table",
    "A base plate 120 (X) by 80 (Y) by 3 mm, spanning x 0..120, y 0..80, z -3..0, as one body. On it stand cylindrical standoffs, "
    "each a separate body (not joined to the plate), standing on z = 0 along Z, centred at (x, y) with the height and diameter from "
    "this CSV table:\n" + csv,
    build_table,
    volume=120 * 80 * 3 + sum(math.pi * (dd / 2) ** 2 * h for _, _, _, h, dd in table),
)

# 4. An optimum.
xo = round((1280 - math.sqrt(1280**2 - 4 * 12 * 24000)) / 24, 2)


def build_opt(d):
    d.cmd.solid.box(length=200 - 2 * xo, width=120 - 2 * xo, height=xo, corner=[0, 0, 0], body_name="block")


task(
    "optbox",
    "One solid block with its minimum corner at the origin, (200 − 2x) mm long in X, (120 − 2x) mm wide in Y and x mm high in Z, "
    "where x is the value that makes the block's volume as large as possible. Round x to 0.01 mm before building.",
    build_opt,
    volume=(200 - 2 * xo) * (120 - 2 * xo) * xo,
)

# 5. A convex hull.
pts = np.round(rng.uniform(-30, 30, (25, 2)), 1)
hull = pts[ConvexHull(pts).vertices]


def build_hull(d):
    extrude_polygon(d, hull, 6, "hull")


task(
    "hull",
    "One solid plate 6 mm thick (z 0..6) whose outline is the convex hull of these 25 points (x, y in mm): "
    + ", ".join(f"({x:g}, {y:g})" for x, y in pts)
    + ". The outline is the hull polygon itself (straight edges between hull points).",
    build_hull,
    volume=6 * polygon_area(hull),
)

# 6. Terraces from a formula.
heights = [round(20 * math.exp(-0.3 * k) + 5, 2) for k in range(10)]


def build_terraces(d):
    d.cmd.solid.box(length=10, width=30, height=heights[0], corner=[0, 0, 0], body_name="terraces")
    for k in range(1, 10):
        d.cmd.solid.box(length=10, width=30, height=heights[k], corner=[10 * k, 0, 0], operation="join", targets=["terraces"])


task(
    "terraces",
    "One solid made of 10 blocks side by side along X: block k (k = 0..9) spans x 10k..10k+10, y 0..30 and z 0..h_k, "
    "where h_k = 20·e^(−0.3k) + 5 mm, rounded to 0.01 mm. The blocks are joined into one body.",
    build_terraces,
    volume=sum(10 * 30 * h for h in heights),
)

# 7. A turned part revolved from sampled radii.
zs = list(range(0, 55, 5))
rs = [20.0, 20.0, 17.5, 14.0, 12.0, 12.0, 15.5, 18.0, 18.0, 11.0, 8.0]
profile = [(0.0, 0.0)] + list(zip(rs, map(float, zs))) + [(0.0, 50.0)]


def build_turned(d):
    sk = d.cmd.sketch.create(plane={"origin": [0, 0, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]})["sketch"]
    d.cmd.sketch.line(points=[[r, z] for r, z in profile], closed=True)
    d.cmd.sketch.finish()
    d.cmd.solid.revolve(sketch=sk, axis="Z", angle=360, body_name="turned")


frustums = sum(math.pi * 5 / 3 * (rs[i] ** 2 + rs[i] * rs[i + 1] + rs[i + 1] ** 2) for i in range(10))
task(
    "turned",
    "A turned part, one solid, made by revolving a profile 360° about the Z axis. The profile, in the XZ plane (x = radius), is the "
    "closed polygon through (0, 0), then (r, z) for each row of this table in order, then (0, 50):\n"
    "z,r\n" + "\n".join(f"{z},{r:g}" for z, r in zip(zs, rs)),
    build_turned,
    volume=frustums,
)

# 8. Sized by a rule.
s = math.ceil((4 * 500 * 200**3 / 70000) ** 0.25)


def build_beam(d):
    d.cmd.solid.box(length=200, width=s, height=s, corner=[0, 0, 0], body_name="beam")


task(
    "beam",
    "A cantilever beam of square cross-section s × s mm, 200 mm long along X: x 0..200, y 0..s, z 0..s, one solid. It is aluminium "
    "(E = 70000 N/mm²) and carries an end load P = 500 N. Use the smallest s, in whole millimetres, for which the tip deflection "
    "P·L³ / (3·E·I), with I = s⁴/12, is at most 1 mm.",
    build_beam,
    volume=200 * s * s,
)

(HERE / "tasks").mkdir(exist_ok=True)
(HERE / "tasks" / "numeric.json").write_text(json.dumps(TASKS, indent=1) + "\n")
print(f"wrote {len(TASKS)} tasks")
