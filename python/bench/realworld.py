"""Real-world tasks: platforms with design freedom and hard constraints from off-the-shelf parts.

Each task gives the parts' dimensions as a datasheet would, and leaves the rest of the design to
the model. The checker (checker.py) scores constraints, not one exact shape: holes where the
parts' screws go, regions that must be empty (a battery, a servo) or full of material (a motor
pad, an arm), limits on mass and size, and, for the wing, its sections against the shape the spec
defines. Every task has a reference build here that must pass every check, and wrong builds that
must fail (tests: `python bench/realworld.py --selftest`).

The parts are generic, described by their public standard dimensions (hole patterns, sizes);
no product is named and no vendor data is used.
"""

from __future__ import annotations

import math
from pathlib import Path
from typing import Any, Callable

# ---------------------------------------------------------------- the airfoil


def naca4(code: str = "2415", n: int = 60) -> list[tuple[float, float]]:
    """NACA 4-digit section from its published equations, unit chord, Selig order (trailing edge,
    upper surface, leading edge, lower surface, trailing edge), cosine-spaced."""
    m, p, t = int(code[0]) / 100, int(code[1]) / 10, int(code[2:]) / 100
    xs = [0.5 * (1 - math.cos(math.pi * i / n)) for i in range(n + 1)]

    def camber(x: float) -> tuple[float, float]:
        if m == 0:
            return 0.0, 0.0
        if x < p:
            return m / p**2 * (2 * p * x - x * x), 2 * m / p**2 * (p - x)
        return m / (1 - p) ** 2 * ((1 - 2 * p) + 2 * p * x - x * x), 2 * m / (1 - p) ** 2 * (p - x)

    upper, lower = [], []
    for x in xs:
        yt = 5 * t * (0.2969 * math.sqrt(x) - 0.1260 * x - 0.3516 * x**2 + 0.2843 * x**3 - 0.1036 * x**4)  # closed trailing edge
        yc, dyc = camber(x)
        th = math.atan(dyc)
        upper.append((x - yt * math.sin(th), yc + yt * math.cos(th)))
        lower.append((x + yt * math.sin(th), yc - yt * math.cos(th)))
    pts = list(reversed(upper)) + lower[1:]
    return [(round(x, 6), round(y, 6)) for x, y in pts]


AIRFOIL = naca4("2415")
AIRFOIL_DAT = "NACA 2415\n" + "\n".join(f"{x:.6f} {y:.6f}" for x, y in AIRFOIL)

# ---------------------------------------------------------------- probes and checks
#
# A probe is a solid the checker builds: {"box": [[x0, y0, z0], [x1, y1, z1]]},
# {"cyl": {"base": [x, y, z], "axis": "X"|"Y"|"Z", "d": D, "h": H}}, or
# {"ring": {"base", "axis", "d_out", "d_in", "h"}}, optionally {"rotate_z": degrees} about the origin.
# A check is (kind, name, args):
#   ("bodies", n)                     exactly n solid bodies
#   ("bbox_within", [[lo], [hi]])     the design's bounding box lies inside
#   ("bbox_spans", axis, lo, hi, tol) the design spans lo..hi along axis, within tol
#   ("volume_max", v)                 total volume at most v
#   ("hole", point, axis, d)          a cylindrical face of diameter d (±0.1) whose axis passes within
#                                     0.1 mm of point and is parallel to axis
#   ("empty", probe)                  no more than 1% of the probe (or 1 mm³) is material
#   ("full", probe)                   at least 98% of the probe is material
#   ("section", y, tol)               at span station y, the design's section follows the spec's airfoil
#                                     within ±tol mm: thin probes just inside its upper and lower
#                                     surfaces are solid, and probes just outside are empty


def box(lo, hi) -> dict:
    return {"box": [list(lo), list(hi)]}


def cyl(base, axis, d, h) -> dict:
    return {"cyl": {"base": list(base), "axis": axis, "d": d, "h": h}}


def ring(base, axis, d_out, d_in, h) -> dict:
    return {"ring": {"base": list(base), "axis": axis, "d_out": d_out, "d_in": d_in, "h": h}}


# ---------------------------------------------------------------- 1. quadcopter frame

QUAD_SPEC = """Design the bottom plate of a 5-inch quadcopter X frame as one solid, cut from 5 mm carbon fibre plate: it lies flat from z = 0 to z = 5, centred on the origin.

Parts it must take (datasheet dimensions, mm):
- Four motors, centred at (80, 80), (-80, 80), (-80, -80) and (80, -80). Each motor base is a 28 mm diameter disc and bolts down with four M3 screws on a 16 x 16 mm square pattern centred on the motor (sides along X and Y), through 3.2 mm clearance holes; its shaft clip needs a 7 mm hole at the motor centre. The plate must be solid under each motor's whole 28 mm base disc, apart from those holes.
- A flight controller stack at the centre: four M3 screws on a 30.5 x 30.5 mm square pattern centred on the origin (sides along X and Y), 3.2 mm clearance holes.
- A battery strapped on top: two strap slots through the plate, each 3 mm (X) by 22 mm (Y), centred at (25, 0) and (-25, 0).

Requirements:
- Each arm, from the centre body out to its motor, is at least 14 mm wide over its whole length.
- Mass at most 90 g (carbon fibre plate: 1.6 g/cm3), so remove material where you can.
- The frame stays within x and y -130..130.
"""


def quad_checks() -> list:
    c: list = [("bodies", 1), ("bbox_spans", "z", 0, 5, 0.01), ("bbox_within", [[-130, -130, -0.01], [130, 130, 5.01]]), ("volume_max", 90 / 1.6e-3)]
    for sx in (1, -1):
        for sy in (1, -1):
            mx, my = 80 * sx, 80 * sy
            c.append(("hole", [mx, my, 2.5], "Z", 7.0))
            for dx in (8, -8):
                for dy in (8, -8):
                    c.append(("hole", [mx + dx, my + dy, 2.5], "Z", 3.2))
            c.append(("full", ring([mx, my, 0], "Z", 28, 26.2, 5)))
            c.append(("full", {**box([25, -7, 0], [100, 7, 5]), "rotate_z": math.degrees(math.atan2(sy, sx))}))
    for x in (15.25, -15.25):
        for y in (15.25, -15.25):
            c.append(("hole", [x, y, 2.5], "Z", 3.2))
    for x in (25, -25):
        c.append(("empty", box([x - 1.4, -10.5, 0], [x + 1.4, 10.5, 5])))
    return c


def quad_reference(d, wrong: str | None = None) -> None:
    arm_w = 12 if wrong == "thin_arms" else 16
    d.cmd.solid.box(length=56, width=56, height=5, center=[0, 0, 2.5], body_name="frame")
    for sx in (1, -1):
        for sy in (1, -1):
            ang = math.degrees(math.atan2(sy, sx))
            d.cmd.solid.box(length=108, width=arm_w, height=5, center=[54, 0, 2.5], body_name="arm")
            d.cmd.solid.move(bodies=["arm"], axis=[0, 0, 1], angle=f"{ang} deg")
            d.cmd.solid.combine(target="frame", tools=["arm"], operation="join")
            d.cmd.solid.cylinder(height=5, diameter=30, base=[80 * sx, 80 * sy, 0], operation="join", targets=["frame"])
    for sx in (1, -1):
        for sy in (1, -1):
            mx, my = 80 * sx, 80 * sy
            d.cmd.solid.cylinder(height=5, diameter=7, base=[mx, my, 0], operation="cut", targets=["frame"])
            for dx in (8, -8):
                for dy in (8, -8):
                    hx = dx * 19 / 16 if wrong == "wrong_pattern" else dx  # a 19 x 16 pattern instead of 16 x 16
                    d.cmd.solid.cylinder(height=5, diameter=3.2, base=[mx + hx, my + dy, 0], operation="cut", targets=["frame"])
    for x in (15.25, -15.25):
        for y in (15.25, -15.25):
            d.cmd.solid.cylinder(height=5, diameter=3.2, base=[x, y, 0], operation="cut", targets=["frame"])
    for x in (25, -25):
        d.cmd.solid.box(length=3, width=22, height=5, center=[x, 0, 2.5], operation="cut", targets=["frame"])


# ---------------------------------------------------------------- 2. flying wing

WING_ROOT, WING_TIP, WING_SEMI, WING_SWEEP, WING_TWIST = 260.0, 140.0, 450.0, 25.0, 2.0
TIP_LE = WING_SEMI * math.tan(math.radians(WING_SWEEP))

WING_SPEC = f"""Design a 900 mm span flying wing as one solid, symmetric about the XZ plane. X points aft, Y along the span, Z up.

Planform and section:
- The airfoil is the section whose coordinates (unit chord, Selig order: trailing edge, upper surface, leading edge, lower surface) are given below. It is used at the root and the tips, chord line along X.
- Root section at y = 0: chord 260 mm, leading edge at (0, 0, 0).
- Tip sections at y = 450 and y = -450: chord 140 mm, leading edge swept back 25 degrees (leading edge at x = 450 tan 25 deg = {TIP_LE:.2f}), and twisted 2 degrees nose-down (washout) about the tip's quarter-chord point, which lies at z = 0.
- Between the root and each tip, the surface is a straight-line (ruled) blend of the two sections.

Parts it must take (mm):
- A battery bay for a 100 x 35 x 22 mm pack: the space x 35..135, y -17.5..17.5, z -8..14 is empty, with at least 1.5 mm of wing material below it and on its front, back and two sides (the top may open as a hatch).
- Two elevon servo pockets, each 23 (X) x 12.5 (Y) x 22 (Z), centred at x = 168.9, y = +150 and y = -150, z = 3: empty (they may open to the surface).
- A pusher motor mount: a firewall block at the root trailing edge whose rear face is the plane x = 272, solid over at least y -20..20 and z -15..15. The motor bolts to it with four M3 screws on a 16 (Y) x 19 (Z) mm pattern centred at (y, z) = (0, 0), through 3.2 mm holes along X at least 8 mm deep, and its shaft boss needs a 10 mm hole along X at (0, 0).

Airfoil coordinates:
{AIRFOIL_DAT}
"""


def wing_checks() -> list:
    c: list = [("bodies", 1), ("bbox_within", [[-1, -451, -40], [TIP_LE + WING_TIP + 2, 451, 40]]), ("bbox_spans", "y", -450, 450, 0.5)]
    for y in (30, 225, -227, 428):
        c.append(("section", y, 0.4))
    c.append(("empty", box([35, -17.5, -8], [135, 17.5, 14])))
    for probe in (box([35, -17.5, -9.5], [135, 17.5, -8]), box([33.5, -17.5, -8], [35, 17.5, 14]), box([135, -17.5, -8], [136.5, 17.5, 14]), box([35, 17.5, -8], [135, 19, 14]), box([35, -19, -8], [135, -17.5, 14])):
        c.append(("full", probe))
    for y in (150, -150):
        c.append(("empty", box([157.4, y - 6.25, -8], [180.4, y + 6.25, 14])))
    for y0, y1 in ((-20, -11), (11, 20)):
        c.append(("full", box([262, y0, -15], [272, y1, 15])))
    for y in (8, -8):
        for z in (9.5, -9.5):
            c.append(("hole", [268, y, z], "X", 3.2))
    c.append(("hole", [268, 0, 0], "X", 10.0))
    return c


def wing_section(y: float, root_chord: float = WING_ROOT, twist: float = WING_TWIST) -> tuple[list, list]:
    """The spec's section at span station y: (upper, lower) as [(x, z)] from leading to trailing edge.
    A ruled blend joins matching points of the root and tip sections with straight lines, so the
    section at y is their point-by-point interpolation."""
    s = abs(y) / WING_SEMI
    th = math.radians(twist)
    q = TIP_LE + 0.25 * WING_TIP
    pts = []
    for u, v in AIRFOIL:
        rx, rz = u * root_chord, v * root_chord
        du, dv = (u - 0.25) * WING_TIP, v * WING_TIP
        tx, tz = q + du * math.cos(th) - dv * math.sin(th), du * math.sin(th) + dv * math.cos(th)
        pts.append(((1 - s) * rx + s * tx, (1 - s) * rz + s * tz))
    le = min(range(len(pts)), key=lambda i: pts[i][0])
    upper = list(reversed(pts[: le + 1]))
    lower = pts[le:]
    return upper, lower


def surface_z(curve: list, x: float) -> float:
    for (x0, z0), (x1, z1) in zip(curve, curve[1:]):
        if x0 <= x <= x1 and x1 > x0:
            return z0 + (z1 - z0) * (x - x0) / (x1 - x0)
    raise ValueError(f"x {x} outside the section")


def wing_outer(d, name: str = "wing", root_chord: float = WING_ROOT, twist: float = WING_TWIST) -> str:
    """The spec's outer wing (no bay, pockets or firewall): two ruled lofts, root to each tip."""
    def section(y: float, chord: float, le_x: float, angle: float) -> Any:
        sk = d.cmd.sketch.create(plane={"origin": [0, y, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]})["sketch"]
        quarter = le_x + 0.25 * chord
        d.cmd.sketch.insert_airfoil(text=AIRFOIL_DAT, chord=chord, pivot=0.25, at=[quarter, 0], angle=angle)
        d.cmd.sketch.finish()
        return sk

    root = section(0, root_chord, 0, 0)
    parts = []
    for side, y in (("R", WING_SEMI), ("L", -WING_SEMI)):
        tip = section(y, WING_TIP, TIP_LE, twist)  # CCW in this sketch lowers the leading edge
        d.call("solid.loft", sections=[{"sketch": root}, {"sketch": tip}], body_name=f"{name}{side}")
        parts.append(f"{name}{side}")
    d.cmd.solid.combine(target=parts[0], tools=[parts[1]], operation="join")
    d.call("body.rename", body=parts[0], name=name)
    return name


def wing_reference(d, wrong: str | None = None) -> None:
    wing_outer(d, "wing", root_chord=280 if wrong == "long_root" else WING_ROOT, twist=0 if wrong == "no_washout" else WING_TWIST)
    d.cmd.solid.box(length=20, width=40, height=30, corner=[252, -20, -15], operation="join", targets=["wing"])
    d.cmd.solid.box(length=100, width=35, height=22, corner=[35, -17.5, -8], operation="cut", targets=["wing"])
    if wrong != "no_hatch_floor":
        pass
    else:
        d.cmd.solid.box(length=100, width=35, height=40, corner=[35, -17.5, -30], operation="cut", targets=["wing"])
    for y in (150, -150):
        d.cmd.solid.box(length=23, width=12.5, height=22, center=[168.9, y, 3], operation="cut", targets=["wing"])
    for y in (8, -8):
        for z in (9.5, -9.5):
            d.cmd.solid.cylinder(height=12, diameter=3.2, base=[272, y, z], axis=[-1, 0, 0], operation="cut", targets=["wing"])
    d.cmd.solid.cylinder(height=12, diameter=10, base=[272, 0, 0], axis=[-1, 0, 0], operation="cut", targets=["wing"])


# ---------------------------------------------------------------- 3. quadruped body

QUADRUPED_SPEC = """Design the body shell of a small quadruped robot as one solid: an open-top box 220 (X) x 110 (Y) x 60 (Z) mm outside, spanning x 0..220, y 0..110, z 0..60, with 3 mm walls and a 3 mm floor (you may add ribs or lightening cut-outs, but keep the walls and floor solid where the parts below need them).

Parts it must take (datasheet dimensions, mm):
- Four standard-size hip servos mount through the end walls, output shaft along X: two through the front wall (x = 0..3) and two through the rear wall (x = 217..220), centred at y = 27.5 and y = 82.5, z = 30. Each needs a rectangular cut-out 20.4 (Y) x 40.4 (Z) through the wall, centred there, and four M3 holes (3.2 mm) through the wall along X on the servo's 10 x 49.5 mm tab pattern: at (y +-5, z +-24.75) from the cut-out centre.
- A controller board on four standoffs: each standoff is a 6 mm diameter post standing on the floor from z = 3 to z = 11, with a 2.5 mm hole along Z for a self-tapping M3 screw, centred at (87.5, 52.5), (132.5, 52.5), (87.5, 97.5) and (132.5, 97.5) (a 45 x 45 mm pattern).
- A 100 x 34 x 26 mm battery lies on the floor in the space x 60..160, y 6..40, z 3..29, which must stay empty.
"""


def quadruped_checks() -> list:
    c: list = [("bodies", 1), ("bbox_within", [[-0.01, -0.01, -0.01], [220.01, 110.01, 60.01]]), ("bbox_spans", "z", 0, 60, 0.01), ("bbox_spans", "x", 0, 220, 0.01)]
    c += [("full", box([10, 0, 5], [210, 3, 55])), ("full", box([10, 107, 5], [210, 110, 55])), ("full", box([45, 10, 0], [175, 45, 3]))]
    for x0, x1 in ((0, 3), (217, 220)):
        c.append(("full", box([x0, 40, 10], [x1, 70, 50])))
        for yc in (27.5, 82.5):
            c.append(("empty", box([x0, yc - 10.1, 30 - 20.1], [x1, yc + 10.1, 30 + 20.1])))
            for dy in (5, -5):
                for dz in (24.75, -24.75):
                    c.append(("hole", [(x0 + x1) / 2, yc + dy, 30 + dz], "X", 3.2))
    for x in (87.5, 132.5):
        for y in (52.5, 97.5):
            c.append(("hole", [x, y, 8], "Z", 2.5))
            c.append(("full", ring([x, y, 4], "Z", 5.8, 2.9, 6)))
    c.append(("empty", box([60, 6, 3], [160, 40, 29])))
    return c


def quadruped_reference(d, wrong: str | None = None) -> None:
    d.cmd.solid.box(length=220, width=110, height=60, corner=[0, 0, 0], body_name="body")
    d.cmd.solid.box(length=214, width=104, height=57, corner=[3, 3, 3], operation="cut", targets=["body"])
    for x0 in (0, 217):
        for yc in (27.5, 82.5):
            d.cmd.solid.box(length=3, width=20.4, height=40.4, center=[x0 + 1.5, yc, 30], operation="cut", targets=["body"])
            for dy in (5, -5):
                for dz in (24.75, -24.75):
                    d.cmd.solid.cylinder(height=3, diameter=3.2, base=[x0, yc + dy, 30 + dz], axis="X", operation="cut", targets=["body"])
    pattern = [(87.5, 52.5), (132.5, 52.5), (87.5, 97.5), (132.5, 97.5)] if wrong != "board_shift" else [(87.5, 42.5), (132.5, 42.5), (87.5, 87.5), (132.5, 87.5)]
    for x, y in pattern:
        d.cmd.solid.cylinder(height=8, diameter=6, base=[x, y, 3], operation="join", targets=["body"])
        d.cmd.solid.cylinder(height=8, diameter=2.5, base=[x, y, 3], operation="cut", targets=["body"])


# ---------------------------------------------------------------- 4. RC car chassis

GEAR_MODULE, SPUR_T, PINION_T = 0.6, 87, 20
MESH = GEAR_MODULE * (SPUR_T + PINION_T) / 2
SPUR_X, SPUR_Z = 230.0, 20.0
MOTOR_X = SPUR_X - MESH

RCCAR_SPEC = """Design the chassis of a 1/10 scale electric RC car as one solid: a flat plate 3 mm thick (z 0..3) of your chosen outline, with a vertical motor mount plate standing on it.

Layout (mm): X points rearward, the front axle is at x = 0 and the rear axle at x = 260, Y across, Z up. The chassis stays within x -40..300 and y -60..60.

Parts it must take (datasheet dimensions, mm):
- A 2S hard-case battery 139 x 47 x 25 lies on the plate in the space x 60..199, y -23.5..23.5, z 3..28, which must stay empty; the plate under it is solid.
- The rear axle's spur gear (87 teeth, module 0.6) turns on a shaft along Y at x = 230, z = 20, carried by an 11 mm bearing seat: an 11 mm hole along Y through the motor mount plate.
- A 540-size motor drives the spur through a 20-tooth module 0.6 pinion on its shaft, which is along Y, at the same height as the spur shaft (z = 20) and forward of it (smaller x), at the gear mesh distance from the spur shaft. The motor bolts to the mount plate with two M3 screws 25 mm apart, on a line along X through the motor axis, centred on it (3.2 mm holes along Y), and its front boss needs a 13 mm hole along Y on the motor axis.
- The motor mount plate is a wall perpendicular to Y, at least 4 mm thick, spanning y 30..34 (at least), from the chassis plate up to z = 38 (at least), and from x = 180 to x = 245 (at least).
"""


def rccar_checks() -> list:
    c: list = [("bodies", 1), ("bbox_within", [[-40, -60, -0.01], [300, 60, 45]]), ("bbox_spans", "z", 0, 38, 7.01)]
    c.append(("full", box([60, -23.5, 0], [199, 23.5, 3])))
    c.append(("empty", box([60, -23.5, 3], [199, 23.5, 28])))
    c.append(("hole", [SPUR_X, 32, SPUR_Z], "Y", 11.0))
    c.append(("hole", [MOTOR_X, 32, SPUR_Z], "Y", 13.0))
    for dx in (12.5, -12.5):
        c.append(("hole", [MOTOR_X + dx, 32, SPUR_Z], "Y", 3.2))
    for x0, x1 in ((180, 183), (238, 245)):
        c.append(("full", box([x0, 30, 5], [x1, 34, 38])))
    return c


def rccar_reference(d, wrong: str | None = None) -> None:
    mx = MOTOR_X if wrong != "mesh_guess" else SPUR_X - 30
    d.cmd.solid.box(length=320, width=100, height=3, corner=[-30, -50, 0], body_name="chassis")
    d.cmd.solid.box(length=70, width=4, height=37, corner=[178, 30, 3], operation="join", targets=["chassis"])
    d.cmd.solid.cylinder(height=4, diameter=11, base=[SPUR_X, 30, SPUR_Z], axis="Y", operation="cut", targets=["chassis"])
    d.cmd.solid.cylinder(height=4, diameter=13, base=[mx, 30, SPUR_Z], axis="Y", operation="cut", targets=["chassis"])
    for dx in (12.5, -12.5):
        d.cmd.solid.cylinder(height=4, diameter=3.2, base=[mx + dx, 30, SPUR_Z], axis="Y", operation="cut", targets=["chassis"])


TASKS: dict[str, dict] = {
    "quadcopter": {"spec": QUAD_SPEC, "checks": quad_checks, "reference": quad_reference, "wrong": ["thin_arms", "wrong_pattern"]},
    "flying_wing": {"spec": WING_SPEC, "checks": wing_checks, "reference": wing_reference, "wrong": ["long_root", "no_washout", "no_hatch_floor"]},
    "quadruped": {"spec": QUADRUPED_SPEC, "checks": quadruped_checks, "reference": quadruped_reference, "wrong": ["board_shift"]},
    "rc_car": {"spec": RCCAR_SPEC, "checks": rccar_checks, "reference": rccar_reference, "wrong": ["mesh_guess"]},
}

Builder = Callable[..., None]


def selftest(only: list[str] | None = None, out_dir: str | None = None) -> int:
    """Each reference build passes every check; each wrong build fails at least one."""
    import tempfile

    import solidcraft as sc
    from checker import check_step

    folder = Path(out_dir or tempfile.mkdtemp(prefix="realworld-"))
    folder.mkdir(parents=True, exist_ok=True)
    bad = 0
    for name, t in TASKS.items():
        if only and name not in only:
            continue
        for wrong in [None, *t["wrong"]]:
            step = folder / f"{name}-{wrong or 'reference'}.step"
            with sc.headless(allow_paths=[str(folder)]) as d:
                t["reference"](d, wrong)
                d.export(path=str(step))
            r = check_step(name, str(step))
            fails = [x for x in r["results"] if not x["ok"]]
            good = r["pass"] if wrong is None else not r["pass"]
            bad += not good
            print(f"{'ok ' if good else 'BAD'} {name} {wrong or 'reference'}: {r.get('passed')}/{r.get('total')} {r.get('error', '')}")
            for x in fails[:6]:
                print(f"      fail {x['check']} -> {x['detail']}")
    return bad


if __name__ == "__main__":
    import sys

    if "--selftest" in sys.argv:
        sys.exit(1 if selftest([a for a in sys.argv[1:] if not a.startswith("--")]) else 0)
    for name, t in TASKS.items():
        print(f"== {name} ({len(t['checks']())} checks, {len(t['spec'])} characters)")
