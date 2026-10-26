"""The kernel complexity ladder's cases (tests/kernel-cases/ladder): every recipe and every
expected answer, with its derivation in `method`. Plain Python 3, standard library only.

    python3 tools/kernel-ladder/cases.py

rewrites each case's recipe.json and expected.json and adds new cases to ladder.json (at the end
of their tier, until `cargo xtask kernel-cases --ladder --record` sorts them by time). It never
touches golden.json, and keeps the recorded order of cases already listed. Never change a value
to match a kernel's output: if they disagree, find out which one is wrong."""
import json
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ladder_lib import *  # noqa: E402,F403
from reported import LOFT_RIBS_PAD, PLEATED_SKINS  # noqa: E402

def up(tier, name):
    return f"../../{tier}/{name}"

T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11 = (
    "01-primitives", "02-booleans", "03-analytic-profiles", "04-spline-profiles", "05-holes-and-taps",
    "06-threads", "07-fillets", "08-chamfers", "09-shells-and-offsets", "10-patterns-and-mirrors", "11-cumulative-parts")
BOX = box([0, 0, 0], 60, 40, 20, body_name="Part")
VB = 60 * 40 * 20
BBOX = {"min": [0, 0, 0], "max": [60, 40, 20]}

# ---------------------------------------------------------------- 01 primitives
case(T1, "001-box", "a 60x40x20 box: the stock every later tier starts from", [BOX],
     {"body_count": 1, "volume_mm3": VB, "area_mm2": 2 * (2400 + 1200 + 800), "faces": 6, "edges": 12, "vertices": 8, "bbox": BBOX},
     "V = 60*40*20; A = 2(60*40 + 60*20 + 40*20); a box has 6 faces, 12 edges, 8 vertices")
case(T1, "002-cylinder", "a cylinder r10 h30", [cyl([0, 0, 0], 10, 30)],
     {"body_count": 1, "volume_mm3": PI * 100 * 30, "area_mm2": 2 * PI * 100 + 2 * PI * 10 * 30, "faces": 3,
      "bbox": {"min": [-10, -10, 0], "max": [10, 10, 30]}},
     "V = pi r^2 h; A = 2 pi r^2 + 2 pi r h; faces: two disks and one whole cylinder")
l = math.hypot(10, 30)
case(T1, "003-cone", "a cone r10 h30 (no cone primitive: a revolved right triangle, its axis a sketch line)",
     sketch(vplane([0, 0, 0]), line([[0, 0], [10, 0], [0, 30]], ids=["b", "s", "ax"], closed=True)) + [C("solid.revolve", axis="ax")],
     {"body_count": 1, "volume_mm3": PI * 100 * 30 / 3, "area_mm2": PI * 100 + PI * 10 * l, "faces": 2,
      "bbox": {"min": [-10, -10, 0], "max": [10, 10, 30]}},
     "V = pi r^2 h / 3; A = pi r^2 + pi r sqrt(r^2 + h^2); a disk and the cone face")
R, r, h = 10, 5, 20
l = math.hypot(R - r, h)
case(T1, "004-frustum", "a cone frustum r10 -> r5, h20 (revolved trapezoid)",
     sketch(vplane([0, 0, 0]), line([[0, 0], [10, 0], [5, 20], [0, 20]], ids=["b", "s", "t", "ax"], closed=True)) + [C("solid.revolve", axis="ax")],
     {"body_count": 1, "volume_mm3": PI * h / 3 * (R * R + R * r + r * r), "area_mm2": PI * (R * R + r * r) + PI * (R + r) * l, "faces": 3},
     "V = pi h/3 (R^2 + R r + r^2); A = pi (R^2 + r^2) + pi (R + r) sqrt((R-r)^2 + h^2)")
case(T1, "005-sphere", "a sphere r10", [C("solid.sphere", radius=10)],
     {"body_count": 1, "volume_mm3": 4 / 3 * PI * 1000, "area_mm2": 4 * PI * 100, "faces": 1,
      "bbox": {"min": [-10, -10, -10], "max": [10, 10, 10]}},
     "V = 4/3 pi r^3; A = 4 pi r^2; one face")
case(T1, "006-torus", "a torus R20 r5", [C("solid.torus", major=20, minor=5)],
     {"body_count": 1, "volume_mm3": 2 * PI * PI * 20 * 25, "area_mm2": 4 * PI * PI * 20 * 5, "faces": 1,
      "bbox": {"min": [-25, -25, -5], "max": [25, 25, 5]}},
     "V = 2 pi^2 R r^2; A = 4 pi^2 R r; one face")
B1 = up(T1, "001-box")

# ---------------------------------------------------------------- 02 booleans
case(T2, "001-coincident-face-join", "a 60x40x10 box joined exactly on the stock's top face: one 60x40x30 box",
     [box([0, 0, 20], 60, 40, 10, operation="join", targets=["Part"])],
     {"body_count": 1, "volume_mm3": 72000.0, "faces": 6, "edges": 12, "vertices": 8, "bbox": {"min": [0, 0, 0], "max": [60, 40, 30]}},
     "the boxes share the whole face z=20 and every side is coplanar: the union is the box 60x40x30, V = 72000, its coplanar sides merged into 6 faces", base=B1)
case(T2, "002-coplanar-overlap-join", "two 60x40x20 boxes overlapping 30x20 in plan, tops and bottoms coplanar",
     [box([30, 20, 0], 60, 40, 20, operation="join", targets=["Part"])],
     {"body_count": 1, "volume_mm3": 2 * VB - 30 * 20 * 20, "faces": 10},
     "V = 2*48000 - 30*20*20; the plan outline is an 8-sided polygon: 8 sides + top + bottom", base=B1)
case(T2, "003-pocket-flush-with-top", "a 20x20x10 pocket whose tool's top is the stock's top face",
     [box([20, 10, 10], 20, 20, 10, operation="cut", targets=["Part"])],
     {"body_count": 1, "volume_mm3": VB - 4000, "faces": 11},
     "V = 48000 - 20*20*10; faces: 6 + 4 pocket walls + pocket floor", base=B1)
case(T2, "004-edge-on-edge-no-overlap", "a cut tool that shares only the top front edge of the stock",
     [box([0, -10, 20], 60, 10, 10, operation="cut", targets=["Part"])],
     {"body_count": 1, "volume_mm3": VB, "faces": 6, "edges": 12, "vertices": 8},
     "the tool touches the stock only along the edge y=0, z=20: nothing is removed, the box stays as it was", base=B1)
case(T2, "005-vertex-on-face-no-overlap", "an inverted cone whose apex touches the middle of the top face, cut",
     sketch(vplane([30, 20, 0]), line([[0, 20], [10, 30], [0, 30]], ids=["s", "t", "ax"], closed=True)) + [C("solid.revolve", axis="ax", operation="cut", targets=["Part"])],
     {"body_count": 1, "volume_mm3": VB, "faces": 6, "edges": 12, "vertices": 8},
     "the cone (apex (30,20,20), base r10 at z=30) meets the stock only at its apex: the cut removes nothing", base=B1)
case(T2, "006-tangent-along-line-no-overlap", "a cylinder r10 along X lying on the top face (tangent along a line), cut",
     [cyl([-5, 20, 30], 10, 70, axis="X", operation="cut", targets=["Part"])],
     {"body_count": 1, "volume_mm3": VB, "faces": 6, "edges": 12, "vertices": 8},
     "the cylinder (axis y=20, z=30) touches the top face z=20 only along the line y=20: nothing is removed", base=B1)
case(T2, "007-near-tangent-gap-1e-3", "a cylinder r10 along X 1e-3 mm above the top face, cut",
     [cyl([-5, 20, 30.001], 10, 70, axis="X", operation="cut", targets=["Part"])],
     {"body_count": 1, "volume_mm3": VB, "faces": 6, "edges": 12, "vertices": 8},
     "gap 1e-3 mm: no contact, nothing is removed", base=B1)
case(T2, "008-near-tangent-gap-1e-6", "a cylinder r10 along X 1e-6 mm above the top face, cut",
     [cyl([-5, 20, 30.000001], 10, 70, axis="X", operation="cut", targets=["Part"])],
     {"body_count": 1, "volume_mm3": VB, "faces": 6, "edges": 12, "vertices": 8},
     "gap 1e-6 mm: no contact; even a kernel that treats it as touching must remove nothing", base=B1)
hh = 1e-3
seg = 100 * math.acos((10 - hh) / 10) - (10 - hh) * math.sqrt(2 * 10 * hh - hh * hh)
case(T2, "009-near-tangent-overlap-1e-3", "a cylinder r10 along X dipping 1e-3 mm into the top face, cut: a groove 1e-3 deep",
     [cyl([-5, 20, 29.999], 10, 70, axis="X", operation="cut", targets=["Part"])],
     {"body_count": 1, "volume_mm3": VB - 60 * seg, "faces": 8},
     f"circular segment of depth h=1e-3 in r=10: A = r^2 acos((r-h)/r) - (r-h) sqrt(2rh - h^2) = {seg:.3e} mm^2, times 60; the groove runs the whole length, so it splits the top face in two and adds one cylinder face: 6 - 1 + 2 + 1", base=B1)
case(T2, "010-near-tangent-overlap-1e-6", "a cylinder r10 along X dipping 1e-6 mm into the top face, cut",
     [cyl([-5, 20, 29.999999], 10, 70, axis="X", operation="cut", targets=["Part"])],
     {"body_count": 1, "volume_mm3": VB},
     "a groove 1e-6 deep (V change ~1e-8) is at the kernels' tolerance: a groove or no change are both right; graded: one valid body, V = 48000", base=B1, source="invariants")
case(T2, "011-tangent-along-circle-through-hole", "a sphere r10 sitting in a through hole r10 at the top edge (tangent along the hole's top circle), cut",
     [cyl([30, 20, -1], 10, 22, operation="cut", targets=["Part"]), C("solid.sphere", radius=10, center=[30, 20, 20], operation="cut", targets=["Part"])],
     {"body_count": 1, "volume_mm3": VB - PI * 100 * 20, "faces": 7},
     "the lower half of the sphere lies inside the hole, tangent to its wall along the circle z=20, the upper half is outside: the sphere removes nothing more; V = 48000 - pi 100 20", base=B1)
case(T2, "012-tangent-along-circle-bowl", "a blind hole r10 10 deep, then a sphere r10 centred on its floor: the floor disk vanishes exactly",
     [cyl([30, 20, 10], 10, 11, operation="cut", targets=["Part"]), C("solid.sphere", radius=10, center=[30, 20, 10], operation="cut", targets=["Part"])],
     {"body_count": 1, "volume_mm3": VB - PI * 100 * 10 - 2 / 3 * PI * 1000, "faces": 8},
     "the sphere's equator is the floor's edge circle (tangent to the hole wall along it): removed = pi r^2 10 + 2/3 pi r^3; faces 6 + wall + bowl", base=B1)
area = quad(lambda y: math.sqrt(100 - y * y) - 5, -2, 2)
case(T2, "013-cut-across-cylinder-seam", "a 10x4 notch through a cylinder r10 across its +X side (where the seam lies)",
     [cyl([0, 0, 0], 10, 20, body_name="Part"), box([5, -2, -1], 10, 4, 22, operation="cut", targets=["Part"])],
     {"body_count": 1, "volume_mm3": PI * 100 * 20 - 20 * area, "faces": 6},
     f"removed section = int_-2^2 (sqrt(100 - y^2) - 5) dy = {area:.9f} (Gauss), times 20; faces: cylinder, 2 disks, 3 notch planes")
def cap(x):
    return quad(lambda y: math.sqrt(100 - x * x - y * y) - 5, -2, 2)
pole = quad(cap, -2, 2)
case(T2, "014-cut-through-sphere-pole", "a 4x4 square post cut off the top of a sphere r10 through its pole",
     [C("solid.sphere", radius=10, body_name="Part"), box([-2, -2, 5], 4, 4, 6, operation="cut", targets=["Part"])],
     {"body_count": 1, "volume_mm3": 4 / 3 * PI * 1000 - pole, "faces": 6},
     f"removed = int over the square |x|,|y|<=2 of (sqrt(100 - x^2 - y^2) - 5) = {pole:.9f} (nested Gauss); faces: sphere, 4 sides, floor z=5")
case(T2, "015-knife-edge-wedge-cut", "a triangular prism cut whose slope meets the bottom face along one line (a knife edge), its other faces on the stock's",
     sketch(plane([0, 41, 0], [1, 0, 0], [0, 0, 1]), line([[0, 0], [60, 20], [0, 20]], closed=True)) + [C("solid.extrude", distance=42, operation="cut", targets=["Part"])],
     {"body_count": 1, "volume_mm3": VB / 2, "faces": 5, "edges": 9, "vertices": 6},
     "the tool removes the half above the plane through the edge x=0,z=0 and the edge x=60,z=20: a wedge 60x40x20/2 = 24000 with a knife edge at x=0, z=0", base=B1)
case(T2, "016-v-groove-pinches-bottom-to-line", "a V groove whose apex touches the bottom face along a line, cut through: two pieces",
     sketch(plane([0, 41, 0], [1, 0, 0], [0, 0, 1]), line([[15, 25], [30, 0], [45, 25]], closed=True)) + [C("solid.extrude", distance=42, operation="cut", targets=["Part"])],
     {"body_count": 2, "volume_mm3": VB - 0.5 * 24 * 20 * 40},
     "apex (30, 0) on the bottom face, flanks through (18, 20) and (42, 20): removed triangle 24 wide x 20 high, times 40; the pieces meet only along the line x=30, z=0, so the regularized result is two solids", base=B1)
case(T2, "017-cylinder-tangent-top-and-bottom", "a cylinder r10 along X tangent to both the top and bottom faces, cut: two pieces with zero-angle cusps",
     [cyl([-5, 20, 10], 10, 70, axis="X", operation="cut", targets=["Part"])],
     {"body_count": 2, "volume_mm3": VB - PI * 100 * 60},
     "the cylinder (axis y=20, z=10) spans the full height at y=20: removed pi r^2 60; the halves meet only along the lines y=20, z=0 and z=20: two solids", base=B1)
case(T2, "018-hole-tangent-to-side-face", "a through hole r10 tangent to the front face: the front face pinched to a line",
     [cyl([30, 10, -1], 10, 22, operation="cut", targets=["Part"])],
     {"body_count": 1, "volume_mm3": VB - PI * 100 * 20},
     "the hole touches y=0 along the line x=30: removed pi r^2 20, the material behind keeps one body", base=B1)
case(T2, "019-notch-flush-with-end-face", "a notch 40x20 through the height, its end flush with the stock's end face: a U",
     [box([20, 10, -1], 40, 20, 22, operation="cut", targets=["Part"])],
     {"body_count": 1, "volume_mm3": VB - 40 * 20 * 20, "faces": 10},
     "the tool's end face is x=60, the stock's: V = 48000 - 16000; the U outline has 8 sides, + top + bottom", base=B1)
case(T2, "020-part-back-wall", "the running part: a 60x10x15 back wall joined on the stock's top, flush with its back and ends",
     [box([0, 30, 20], 60, 10, 15, operation="join", targets=["Part"])],
     {"body_count": 1, "volume_mm3": VB + 60 * 10 * 15, "faces": 8, "bbox": {"min": [0, 0, 0], "max": [60, 40, 35]}},
     "an L section (6 sides) extruded along X: 8 faces; the coplanar back and end faces merge", base=B1)
C2 = up(T2, "020-part-back-wall")
V2 = VB + 9000
case(T2, "021-holes-touching-at-a-point", "two through holes r5 whose circles touch at one point (30, 20)",
     [cyl([25, 20, -1], 5, 22, operation="cut"), cyl([35, 20, -1], 5, 22, operation="cut")],
     {"body_count": 1, "volume_mm3": VB - 2 * PI * 25 * 20, "faces": 8},
     "V = 60 40 20 - 2 pi 5^2 20; faces: 6 of the box + 2 bores (top and bottom each one loop round both holes, through their common vertex twice)", base=B1)
# The tool's square 1.5 x 1.5 outside r 1.6 (the hole wall): A1 = int_x0^1.5 (1.5 - sqrt(1.6^2 - x^2)) dx, x0 = sqrt(1.6^2 - 1.5^2).
x0 = math.sqrt(2.56 - 2.25)
Fc = lambda x: (x * math.sqrt(2.56 - x * x) + 2.56 * math.asin(x / 1.6)) / 2
A1 = 1.5 * (1.5 - x0) - (Fc(1.5) - Fc(x0))
case(T2, "022-cylinder-grazing-a-hole-edge",
     "a corner piece (square column minus a quarter disc r1.5, 0.0003 off the axis) cut out of a part with a blind r1.6 hole on the same axis: the tool's r1.5 face sits 0.1 mm inside the hole wall; 'no result'",
     [box([-6, -6, -5], 12, 12, 5.1, body_name="Part"), cyl([0, 0, -4], 1.6, 4.1, operation="cut"),
      box([0, 0, -3.9], 1.5, 1.5, 7.1, operation="new", body_name="Tool"), cyl([0.0003, 0, -4], 1.5, 7.5, operation="cut", targets=["Tool"]),
      C("solid.combine", target="Part", tools=["Tool"], operation="cut")],
     {"body_count": 1, "volume_mm3": 734.4 - PI * 1.6 ** 2 * 4.1 - A1 * 4.0},
     "part 12x12x5.1 = 734.4 minus the hole pi*1.6^2*4.1 = 32.97 (hole z -4..0.1, part top z 0.1). The tool (z -3.9..3.2, the square 1.5x1.5 where r >= 1.5) overlaps solid only where r >= 1.6: "
     "area A1 = integral_0^1.5 (1.5 - min(1.5, sqrt(2.56-x^2))) dx = 0.31409, over the 4.0 mm of shared z: 1.25638 mm3 (the 0.0003 offset moves this by about 3e-3). "
     "V = 734.4 - 32.9662 - 1.2564 = 700.1695. Reduced from the report's STEP geometry (no pocket or rotation); the answer is for this geometry",
     sources=["tangency-labs/solidcraft#1279"])
def arc_c(center, start, sweep):
    return C("sketch.arc.center_point", center=center, start=start, sweep=sweep)
case(T2, "023-lips-1e-4-off-the-walls-joined",
     "two half-annulus lips (r 4..5, from z=2 to 20) joined onto a stadium-ring base (z 0..2, r 3..5 arcs, centres 20 apart) with their cylinder walls 0.0001 mm off the base's: "
     "the lip cap touches the base top, leaving a 0.0001 mm crescent overhang. Layout is a hand-built variant of the PR's public reproduction",
     [C("sketch.create", plane="XY", name="Base"),
      line([[5, -10], [5, 10]]), arc_c([0, 10], [5, 10], 180), line([[-5, 10], [-5, -10]]), arc_c([0, -10], [-5, -10], 180),
      line([[3, -10], [3, 10]]), arc_c([0, 10], [3, 10], 180), line([[-3, 10], [-3, -10]]), arc_c([0, -10], [-3, -10], 180),
      C("sketch.finish"),
      C("solid.extrude", sketch="Base", profiles=[{"point": [4, 0]}], distance=2, operation="new", body_name="Base"),
      C("sketch.create", plane="XY", offset=2, name="Lips"),
      arc_c([0.0001, 10], [5.0001, 10], 180), line([[-4.9999, 10], [-3.9999, 10]]), arc_c([0.0001, 10], [-3.9999, 10], -180), line([[4.0001, 10], [5.0001, 10]]),
      arc_c([-0.0001, -10], [-5.0001, -10], 180), line([[4.9999, -10], [3.9999, -10]]), arc_c([-0.0001, -10], [3.9999, -10], -180), line([[-4.0001, -10], [-5.0001, -10]]),
      C("sketch.finish"),
      C("solid.extrude", sketch="Lips", distance=18, operation="join", profiles="all")],
     {"body_count": 1, "volume_mm3": 160 + 194 * PI},
     "base area = 2 straight rails (2 x 20) x 2 + full annulus pi(25-9) = 80 + 16 pi, times 2 mm = 160 + 32 pi. Each lip is a half annulus pi(25-16)/2 = 4.5 pi, two lips x 18 mm = 162 pi. "
     "The lips sit above z=2, the base is below, so nothing overlaps (the crescent overhang is above z=2 too): V = 160 + 194 pi = 769.469. One body",
     sources=["tangency-labs/solidcraft#2928"])
case(T2, "024-wall-halves-merged-into-a-flange",
     "the flange top annulus of a freshly revolved hub (z = 6, r 18..30) picked by a point and extruded 5 mm (the pick found a third of the face: \"the selected planar face no longer exists\")",
     sketch(vplane([0.0, 0.0, 0.0]), C("sketch.line", points=[[8, 0], [30, 0], [30, 6], [18, 6], [18, 22], [8, 22]], infer=False, closed=True), name="P")
     + [C("solid.revolve", axis="Z", angle="360 deg", operation="new", body_name="Hub"),
        C("solid.extrude", face=[12, 12 * math.sqrt(3), 6], distance=5, operation="join")],
     {"body_count": 1, "volume_mm3": PI * ((900 - 64) * 6 + (324 - 64) * 16) + PI * (900 - 324) * 5},
     "hub = revolve of (8,0) (30,0) (30,6) (18,6) (18,22) (8,22) about Z: V = pi [(30^2 - 8^2) 6 + (18^2 - 8^2) 16] = 28826.6. "
     "Extruding the whole annulus z = 6, r 18..30 outward (upward) 5 mm adds pi (30^2 - 18^2) 5 = 9047.8 (the new ring sits on the flange and touches the hub wall r = 18).",
     sources=["tangency-labs/solidcraft#726", "tangency-labs/solidcraft#494", "tangency-labs/solidcraft#2114"])
# 18 chords across a disc r37; the layers' regions between them, four bands left out of the second layer.
CHORD_Y = [-32, -29, -24, -21, -16, -13, -8, -5, 0, 3, 8, 11, 16, 19, 24, 27, 32, 35]
BANDS = [(-29, -24), (-13, -8), (8, 11), (24, 27)]
def chords(name):
    return sketch("XY", C("sketch.circle.center", center=[0, 0], radius=37),
                  *[line([[-math.sqrt(1369 - y * y), y], [math.sqrt(1369 - y * y), y]]) for y in CHORD_Y], name=name)
ys = [-37] + CHORD_Y + [37]
picks = [{"point": [0, (a + b) / 2]} for a, b in zip(ys, ys[1:]) if (a, b) not in BANDS]
band_area = sum(quad(lambda y: 2 * math.sqrt(1369 - y * y), a, b) for a, b in BANDS)
case(T2, "025-flush-chord-layers-joined",
     "a join of flush layers: a disc r37 (z -1.2..0) carrying 15 of the 19 regions of a sketch of 18 chords (z -3.6..-1.2; four bands left out), on an annulus r35..37 (z -5.6..-3.6) "
     "joined to a disc of all the chord regions (z -8..-5.6): SMLib's result came back with 'loop encloses no (u, v) area' though both operands are valid",
     chords("S1") + chords("S3")
     + sketch("XY", C("sketch.circle.center", center=[0, 0], radius=37), C("sketch.circle.center", center=[0, 0], radius=35), name="Rings")
     + [C("solid.extrude", sketch="Rings", profiles="all", distance=1.2, direction="negative", body_name="Body1"),
        C("solid.extrude", sketch="S1", profiles=picks, distance=2.4, direction="negative", **{"from": [0, 0, -1.2]}, operation="join"),
        C("solid.extrude", sketch="Rings", profiles=[{"point": [36, 0]}], distance=2, direction="negative", **{"from": [36, 0, -3.6]}, operation="new", body_name="Body2"),
        C("solid.extrude", sketch="S3", profiles="all", distance=2.4, direction="negative", **{"from": [36, 0, -5.6]}, operation="new", body_name="Body3"),
        C("solid.combine", target="Body2", tools=["Body3"], operation="join"),
        C("solid.combine", target="Body1", tools=["Body2"], operation="join")],
     {"body_count": 1, "shells": 1, "volume_mm3": PI * 1369 * 1.2 + (PI * 1369 - band_area) * 2.4 + PI * (1369 - 1225) * 2 + PI * 1369 * 2.4},
     "the layers only meet face to face, so V is their sum: Body1 = pi 37^2 1.2 + (pi 37^2 - the four bands y -29..-24, -13..-8, 8..11, 24..27 of the disc, Int 2 sqrt(37^2 - y^2) dy) 2.4; "
     "Body2 = pi (37^2-35^2) 2; Body3 = pi 37^2 2.4. The bands run out to the rim, so the space inside r35 under Body1 opens to the outside: one shell (the reporter's 52-command script, generic dimensions)",
     sources=["tangency-labs/solidcraft#5197", "tangency-labs/solidcraft#3396"])

# ---------------------------------------------------------------- 03 analytic profiles
A = 2400 + PI * 400 / 2
case(T3, "001-extrude-rect-with-arc-end", "a 60x40 profile with a semicircular end r20, extruded 20",
     sketch("XY", line([[60, 40], [0, 40], [0, 0], [60, 0]]), C("sketch.arc.three_point", start=[60, 0], end=[60, 40], through=[80, 20])) + [C("solid.extrude", distance=20)],
     {"body_count": 1, "volume_mm3": A * 20, "area_mm2": 2 * A + 20 * (60 + 40 + 60 + PI * 20), "faces": 6},
     "A = 60*40 + pi 20^2 / 2; V = 20 A; sides: 3 planes and a half cylinder")
case(T3, "002-extrude-annulus", "an annulus r20/r10 extruded 15",
     sketch("XY", C("sketch.circle.center", center=[0, 0], radius=20), C("sketch.circle.center", center=[0, 0], radius=10)) + [C("solid.extrude", distance=15, profiles=[{"point": [15, 0]}])],
     {"body_count": 1, "volume_mm3": PI * 300 * 15, "area_mm2": 2 * PI * 300 + 2 * PI * 30 * 15, "faces": 4},
     "V = pi (20^2 - 10^2) 15; faces: two annuli, two cylinders")
rt = 10 - 20 * math.tan(math.radians(5))
case(T3, "003-extrude-tapered-circle", "a circle r10 extruded 20 with a -5 deg taper (inward): a cone frustum",
     sketch("XY", C("sketch.circle.center", center=[0, 0], radius=10)) + [C("solid.extrude", distance=20, taper=-5)],
     {"body_count": 1, "volume_mm3": PI * 20 / 3 * (100 + 10 * rt + rt * rt), "faces": 3},
     f"positive taper grows outward (tests/oracle/05): top radius 10 - 20 tan5 = {rt:.9f}; V = pi h/3 (R^2 + R r + r^2)")
case(T3, "004-revolve-full-off-axis", "a 10x30 rectangle 10 mm off the Z axis revolved 360 deg: a tube",
     sketch(vplane([0, 0, 0]), line([[10, 0], [20, 0], [20, 30], [10, 30]], closed=True)) + [C("solid.revolve", axis="Z")],
     {"body_count": 1, "volume_mm3": PI * 300 * 30, "area_mm2": 2 * PI * 300 + 2 * PI * 30 * 30, "faces": 4},
     "V = pi (20^2 - 10^2) 30; faces: two annuli, two cylinders")
case(T3, "005-revolve-touching-axis-hemisphere", "a quarter disk r20 with an edge on the Z axis revolved 360 deg: a hemisphere",
     sketch(vplane([0, 0, 0]), line([[0, 20], [0, 0], [20, 0]]), C("sketch.arc.center_point", center=[0, 0], start=[20, 0], end=[0, 20])) + [C("solid.revolve", axis="Z")],
     {"body_count": 1, "volume_mm3": 2 / 3 * PI * 8000, "area_mm2": 3 * PI * 400, "faces": 2},
     "V = 2/3 pi r^3; A = 2 pi r^2 + pi r^2; the profile edge on the axis makes no face")
case(T3, "006-revolve-partial-90", "a 10x10 rectangle 10 mm off the axis revolved 90 deg",
     sketch(vplane([0, 0, 0]), line([[10, 0], [20, 0], [20, 10], [10, 10]], closed=True)) + [C("solid.revolve", axis="Z", angle=90)],
     {"body_count": 1, "volume_mm3": PI * 300 * 10 / 4, "faces": 6},
     "a quarter of the tube: V = (90/360) pi (20^2 - 10^2) 10; faces: 2 ends, 2 cylinders, top, bottom")
case(T3, "007-revolve-180-touching-axis", "a 15x20 rectangle on the axis revolved 180 deg: a half cylinder",
     sketch(vplane([0, 0, 0]), line([[0, 0], [15, 0], [15, 20], [0, 20]], closed=True)) + [C("solid.revolve", axis="Z", angle=180)],
     {"body_count": 1, "volume_mm3": PI * 225 * 20 / 2, "area_mm2": PI * 225 + PI * 15 * 20 + 30 * 20, "faces": 4},
     "V = pi r^2 h / 2; the two end rectangles are coplanar and meet on the axis: one 30x20 face; + half disks top and bottom + half cylinder")
case(T3, "008-extrude-slot", "a slot (centres 40 apart, width 20) extruded 10",
     sketch("XY", C("sketch.slot.center_to_center", p0=[0, 0], p1=[40, 0], width=20)) + [C("solid.extrude", distance=10)],
     {"body_count": 1, "volume_mm3": (800 + PI * 100) * 10, "area_mm2": 2 * (800 + PI * 100) + 10 * (80 + 2 * PI * 10), "faces": 6},
     "A = 40*20 + pi 10^2; faces: 2 planes, 2 half cylinders, top, bottom")
Vboss = PI * 64 * 10
Aslot = 15 * 8 + PI * 16
case(T3, "009-part-boss-and-slot", "the running part: a revolved boss r8 h10 joined on the top, a slot 15x8 cut through the base",
     sketch(vplane([15, 15, 0]), line([[0, 20], [8, 20], [8, 30], [0, 30]], ids=["b", "s", "t", "ax"], closed=True)) + [C("solid.revolve", axis="ax", operation="join", targets=["Part"], name="Boss")]
     + sketch("XY", C("sketch.slot.center_to_center", p0=[35, 10], p1=[50, 10], width=8)) + [C("solid.extrude", distance=20, operation="cut", targets=["Part"], name="Slot")],
     {"body_count": 1, "volume_mm3": V2 + Vboss - 20 * Aslot, "faces": 14},
     "back-wall part (57000) + pi 8^2 10 - 20 (15*8 + pi 4^2); faces 8 + boss side and top + slot's 2 planes and 2 half cylinders", base=C2)
C3 = up(T3, "009-part-boss-and-slot")
V3 = V2 + Vboss - 20 * Aslot
case(T3, "010-revolve-horn-torus-270",
     "the half disc of radius 14 on the far side of a diameter 14 mm from the axis (the full circle would touch the axis: a horn torus) revolved 270 degrees",
     sketch(vplane([0.0, 0.0, 0.0]), C("sketch.arc.center_point", center=[14, 0], start=[14, -14], end=[14, 14]),
            C("sketch.line", points=[[14, 14], [14, -14]], infer=False), name="P")
     + [C("solid.revolve", axis="Z", angle="270 deg", operation="new")],
     {"body_count": 1, "volume_mm3": 270 / 360 * 2 * PI * (14 + 4 * 14 / (3 * PI)) * PI * 196 / 2},
     "Pappus: the half disc (the side away from the axis) has area pi 14^2 / 2 and centroid at 14 + 4 * 14 / (3 pi) from the axis: "
     "V = (270/360) 2 pi (14 + 4*14/(3 pi)) pi 14^2 / 2 = 28932.2 (the design measures 28930.7).",
     sources=["tangency-labs/solidcraft#4007"])

# ---------------------------------------------------------------- 04 spline profiles
P = [[40, 10], [30, 30], [10, 30], [0, 10]]
Ab = 40 * 10 + bez_area_under(P, 10)
case(T4, "001-extrude-bezier-arch", "a profile closed by a cubic Bezier arch (control-point spline, 4 points), extruded 10",
     sketch("XY", line([[0, 10], [0, 0], [40, 0], [40, 10]]), C("sketch.spline.control_point", points=P)) + [C("solid.extrude", distance=10)],
     {"body_count": 1, "volume_mm3": Ab * 10, "faces": 6, "bbox": {"min": [0, 0, 0], "max": [40, 25, 10]}},
     "a 4-point cubic control-point spline is one Bezier segment; A = 400 + int (y-10) dx over the arch (Green, exact Gauss) = %.9f; max y = 10 + 0.75*20" % Ab)
Q = [[20, 0], [30, 10], [10, 20], [15, 30]]
Vq = PI * quad(lambda t: bez(Q, t)[0] ** 2 * dbez(Q, t)[1], 0, 1)
Aq = PI * 400 + PI * 225 + 2 * PI * quad_split(lambda t: bez(Q, t)[0] * norm(dbez(Q, t)), 0, 1, 50)
case(T4, "002-revolve-bezier-profile", "a profile with a Bezier side (r from 20 to 15 over z 0..30) revolved 360 deg about Z",
     sketch(vplane([0, 0, 0]), line([[15, 30], [0, 30], [0, 0], [20, 0]]), C("sketch.spline.control_point", points=Q)) + [C("solid.revolve", axis="Z")],
     {"body_count": 1, "volume_mm3": Vq, "area_mm2": Aq, "faces": 3},
     "z(t) = 30 t is linear, so V = pi int r(t)^2 z'(t) dt (exact Gauss) = %.9f; A = disks pi 20^2 + pi 15^2 + 2 pi int r |B'| dt" % Vq)
W = [[0, 0], [0, 20], [20, 20], [20, 40]]
Lw = bez_len(W)
assert bez_min_radius(W) > 3
case(T4, "003-pipe-along-bezier", "a pipe d4 along a planar Bezier path",
     sketch(vplane([0, 0, 0]), C("sketch.spline.control_point", points=W), name="Path") + [C("solid.pipe", path_sketch="Path", path=["s1"], diameter=4)],
     {"body_count": 1, "volume_mm3": PI * 4 * Lw, "area_mm2": 2 * PI * 2 * Lw + 2 * PI * 4},
     f"a disk swept square to a planar path whose curvature radius stays above {bez_min_radius(W):.2f} > 2: V = pi r^2 L, A = 2 pi r L + 2 pi r^2, L = {Lw:.9f}")
case(T4, "004-sweep-square-along-bezier", "a 4x4 square swept along the same Bezier path",
     sketch(vplane([0, 0, 0]), C("sketch.spline.control_point", points=W), name="Path")
     + sketch("XY", C("sketch.rectangle.center", center=[0, 0], corner=[2, 2]), name="Square")
     + [C("solid.sweep", sketch="Square", path_sketch="Path", path=["s1"])],
     {"body_count": 1, "volume_mm3": 16 * Lw, "area_mm2": 16 * Lw + 2 * 16},
     "the square's centre is on the planar path and it stays square to it: V = A L; side faces: the inner and outer walls have s(L -/+ a dtheta), the two others s L each, total 4 s L; + 2 ends")
case(T4, "005-loft-square-frustum", "a loft between a 40 square at z=0 and a 20 square at z=30",
     sketch("XY", C("sketch.rectangle.center", center=[0, 0], corner=[20, 20]), name="S0")
     + sketch(plane([0, 0, 30], [1, 0, 0], [0, 1, 0]), C("sketch.rectangle.center", center=[0, 0], corner=[10, 10]), name="S1")
     + [C("solid.loft", sections=[{"sketch": "S0"}, {"sketch": "S1"}])],
     {"body_count": 1, "volume_mm3": 30 / 3 * (1600 + 400 + 800), "area_mm2": 1600 + 400 + 4 * 30 * math.hypot(30, 10), "faces": 6},
     "two sections loft ruled: a square frustum, V = h/3 (A0 + A1 + sqrt(A0 A1)); sides 4 trapezoids (40+20)/2 * sqrt(30^2+10^2)")
case(T4, "006-loft-circle-frustum", "a loft between circles r20 at z=0 and r10 at z=30",
     sketch("XY", C("sketch.circle.center", center=[0, 0], radius=20), name="S0")
     + sketch(plane([0, 0, 30], [1, 0, 0], [0, 1, 0]), C("sketch.circle.center", center=[0, 0], radius=10), name="S1")
     + [C("solid.loft", sections=[{"sketch": "S0"}, {"sketch": "S1"}])],
     {"body_count": 1, "volume_mm3": PI * 30 / 3 * 700, "faces": 3},
     "a ruled loft between coaxial circles is a cone frustum: V = pi h/3 (R^2 + R r + r^2)")
case(T4, "007-loft-three-circles", "a smooth loft through circles r20, r10, r20 at z = 0, 20, 40",
     sketch("XY", C("sketch.circle.center", center=[0, 0], radius=20), name="S0")
     + sketch(plane([0, 0, 20], [1, 0, 0], [0, 1, 0]), C("sketch.circle.center", center=[0, 0], radius=10), name="S1")
     + sketch(plane([0, 0, 40], [1, 0, 0], [0, 1, 0]), C("sketch.circle.center", center=[0, 0], radius=20), name="S2")
     + [C("solid.loft", sections=[{"sketch": "S0"}, {"sketch": "S1"}, {"sketch": "S2"}])],
     {"body_count": 1, "faces": 3},
     "the interpolation between sections is the loft's own: no closed-form volume; one valid body bounded by two disks and one lofted face", source="invariants")
case(T4, "008-loft-square-to-circle", "a loft from a 40 square at z=0 to a circle r10 at z=30",
     sketch("XY", C("sketch.rectangle.center", center=[0, 0], corner=[20, 20]), name="S0")
     + sketch(plane([0, 0, 30], [1, 0, 0], [0, 1, 0]), C("sketch.circle.center", center=[0, 0], radius=10), name="S1")
     + [C("solid.loft", sections=[{"sketch": "S0"}, {"sketch": "S1"}])],
     {"body_count": 1},
     "sections with different edge counts: the matching is the loft's own; one valid body", source="invariants")
case(T4, "009-extrude-periodic-fit-spline", "a closed periodic fit-point spline through 6 points, extruded 10",
     sketch("XY", C("sketch.spline.fit_point", points=[[20, 0], [10, 17], [-10, 17], [-20, 0], [-10, -17], [10, -17]], periodic=True)) + [C("solid.extrude", distance=10)],
     {"body_count": 1},
     "the fit's parametrisation is the spline's own: no closed-form area, and how many side faces a periodic spline makes is the kernel's own; one valid body", source="invariants")
Pw = [[40, 22], [37, 34], [23, 34], [20, 22]]
Awin = bez_area_under(Pw, 22)
case(T4, "010-part-bezier-window", "the running part: a Bezier-arched window cut through the back wall",
     sketch(plane([0, 40, 0], [1, 0, 0], [0, 0, 1]), line([[20, 22], [40, 22]]), C("sketch.spline.control_point", points=Pw)) + [C("solid.extrude", distance=10, operation="cut", targets=["Part"], name="Window")],
     {"body_count": 1, "volume_mm3": V3 - 10 * Awin, "faces": 16},
     "the arch over z=22 from x=20 to 40 rises to z = 22 + 0.75*12 = 31, under the wall top (35): removed 10 * %.9f; + window floor and arch faces" % Awin, base=C3)
C4 = up(T4, "010-part-bezier-window")
V4 = V3 - 10 * Awin
case(T4, "011-crossing-pleated-loft-skins-joined",
     "a join of two pleated skins (lofts between 24-pleat star outlines, less the same loft moved in 1.1 mm) that cross along a ring near their bottoms: "
     "truck panicked ('tolerance must be no less than 1e-6') or gave no result",
     PLEATED_SKINS, {"body_count": 1},
     "the narrowing cone skin and the flaring lip skin cross at z = -0.3, so their union is one connected solid: one valid closed body, no panic. The reporter's own script (pleated_union.py, 24 pleats)",
     source="invariants", sources=["tangency-labs/solidcraft#4623", "tangency-labs/solidcraft#4180", "tangency-labs/solidcraft#4881"])
case(T4, "012-loft-ribs-joined-to-a-rounded-pad", "the same without the top flange: the join fails with 'boolean Union: no result'",
     LOFT_RIBS_PAD, {"body_count": 1},
     "the pad (X -8..8, Y 18..25, Z 36..59) overlaps the body, ribs and ring, so the join is one connected solid; the join must succeed with one valid closed solid and no panic",
     source="invariants", sources=["tangency-labs/solidcraft#4092"])

# ---------------------------------------------------------------- 05 holes and taps
def H(**p):
    return C("solid.hole", **p)
top = [30, 20, 20]
case(T5, "001-simple-through", "a d10 through hole in the stock's top", [H(position=top, diameter=10)],
     {"body_count": 1, "volume_mm3": VB - PI * 25 * 20, "faces": 7}, "removed pi 5^2 20; + one cylinder face", base=B1)
case(T5, "002-simple-blind-flat", "a d10 blind hole 10 deep, flat bottom", [H(position=top, diameter=10, depth=10)],
     {"body_count": 1, "volume_mm3": VB - PI * 25 * 10, "faces": 8}, "removed pi 5^2 10; + wall and floor", base=B1)
ch = 5 / math.tan(math.radians(59))
case(T5, "003-drilled-blind-118", "a d10 drilled hole 10 deep to the shoulder with a 118 deg point",
     [H(position=top, diameter=10, depth=10, type="drilled", tip_angle=118)],
     {"body_count": 1, "volume_mm3": VB - PI * 25 * 10 - PI * 25 * ch / 3, "faces": 8},
     "depth to the shoulder (tests/oracle/18); point cone h = r / tan59; removed pi r^2 10 + pi r^2 h/3; + wall and cone", base=B1)
case(T5, "004-counterbore", "a counterbored hole: d6.6 through, d11 x 6.4 counterbore",
     [H(position=top, diameter=6.6, type="counterbore", cb_diameter=11, cb_depth=6.4)],
     {"body_count": 1, "volume_mm3": VB - PI * 3.3 ** 2 * (20 - 6.4) - PI * 5.5 ** 2 * 6.4, "faces": 9},
     "removed pi 3.3^2 (20 - 6.4) + pi 5.5^2 6.4; + bore, counterbore wall, its floor annulus", base=B1)
hc = (6 - 3.3) / math.tan(math.radians(45))
case(T5, "005-countersink", "a countersunk hole: d6.6 through, d12 x 90 deg countersink",
     [H(position=top, diameter=6.6, type="countersink", cs_diameter=12, cs_angle=90)],
     {"body_count": 1, "volume_mm3": VB - PI * hc / 3 * (36 + 6 * 3.3 + 3.3 ** 2) - PI * 3.3 ** 2 * (20 - hc), "faces": 8},
     "countersink frustum r6 -> r3.3 over (6-3.3)/tan45 = 2.7; + bore below it; + cone and bore faces", base=B1)
case(T5, "006-tapped-m6-through", "an M6 tapped hole through: the tap drill d5 (major - pitch), cosmetic thread",
     [H(position=top, thread="M6")],
     {"body_count": 1, "volume_mm3": VB - PI * 6.25 * 20, "faces": 7}, "M6 coarse pitch 1: tap drill 6 - 1 = 5; removed pi 2.5^2 20; the cosmetic thread adds no geometry", base=B1)
ch8 = 3.375 / math.tan(math.radians(59))
case(T5, "007-tapped-m8-blind", "an M8 tapped blind hole 12 deep with a 118 deg drill point",
     [H(position=top, thread="M8", depth=12, type="drilled", tip_angle=118)],
     {"body_count": 1, "volume_mm3": VB - PI * 3.375 ** 2 * 12 - PI * 3.375 ** 2 * ch8 / 3, "faces": 8},
     "M8 coarse pitch 1.25: tap drill (major - pitch) 6.75; removed pi r^2 12 to the shoulder + the point cone pi r^2 (r/tan59)/3", base=B1)
r, Rc = 2.1, 20
vint = quad(lambda y: 2 * math.sqrt(max(r * r - y * y, 0)) * 2 * math.sqrt(Rc * Rc - y * y), -r, r, 128)
case(T5, "008-tapped-m5-into-curved-face", "an M5 tapped hole drilled radially through a cylinder r20 lying along X",
     [cyl([0, 0, 0], 20, 60, axis="X", body_name="Part"), H(position=[30, 0, 20], thread="M5")],
     {"body_count": 1, "volume_mm3": PI * 400 * 60 - vint, "faces": 4},
     f"tap drill 5 - 0.8 = 4.2; the hole (axis Z) crosses the cylinder (axis X, R=20): V_int = int_-r^r 2 sqrt(r^2 - y^2) 2 sqrt(R^2 - y^2) dy = {vint:.9f}")
case(T5, "009-hole-0.5-from-edge", "a d10 through hole whose wall is 0.5 mm from the end face",
     [H(position=[5.5, 20, 20], diameter=10)],
     {"body_count": 1, "volume_mm3": VB - PI * 25 * 20, "faces": 7}, "removed pi 5^2 20, a 0.5 mm web left", base=B1)
case(T5, "010-hole-tangent-to-end-face", "a d10 through hole tangent to the end face x=0",
     [H(position=[5, 20, 20], diameter=10)],
     {"body_count": 1, "volume_mm3": VB - PI * 25 * 20}, "the wall touches x=0 along a line: removed pi 5^2 20, one body", base=B1)
case(T5, "011-crossing-holes-steinmetz", "two d10 through holes whose axes cross: vertical and along Y at z=10",
     [H(position=top, diameter=10), H(position=[30, 0, 10], diameter=10)],
     {"body_count": 1, "volume_mm3": VB - PI * 25 * 20 - PI * 25 * 40 + 16 * 125 / 3, "faces": 10},
     "equal radii, crossing axes: the common part is the Steinmetz solid 16 r^3 / 3; removed pi r^2 (20 + 40) - 16 r^3/3. "
     "Faces: the stock's 6 and two per hole. With equal radii the walls cross along two ellipses that meet at the two "
     "points where the walls are tangent (x = 25 and 35, y = 20, z = 10), so each wall is left in two pieces (the vertical "
     "one above and below the other hole, the horizontal one either side of the vertical hole) that touch only at those "
     "points: no edge joins them, so they are separate faces", base=B1)
rr, off = 2.5, 2.0
lo, hi = max(-rr, off - rr), min(rr, off + rr)
vx = quad(lambda u: 4 * math.sqrt(max(rr * rr - u * u, 0)) * math.sqrt(max(rr * rr - (u - off) ** 2, 0)), lo, hi, 256)
case(T5, "012-crossing-tapped-holes-offset", "two M6 tapped holes (d5) crossing with axes 2 mm apart",
     [H(position=top, thread="M6"), H(position=[32, 0, 10], thread="M6")],
     {"body_count": 1, "volume_mm3": VB - PI * 6.25 * 20 - PI * 6.25 * 40 + vx},
     f"common part of perpendicular cylinders r=2.5 with axes 2 apart: int 2 sqrt(r^2-u^2) 2 sqrt(r^2-(u-2)^2) du = {vx:.9f}", base=B1)
V5 = V4 - PI * 6.25 * 20 - PI * 2.1 ** 2 * 8 - PI * 1.65 ** 2 * 35
case(T5, "013-part-holes", "the running part: an M6 tapped through hole, an M5 tapped blind hole in the boss, a d3.3 hole down through wall and base",
     [H(position=[52, 22, 20], thread="M6"), H(position=[15, 15, 30], thread="M5", depth=8), H(position=[5, 35, 35], diameter=3.3)],
     {"body_count": 1, "volume_mm3": V5, "faces": 20},
     "removed pi 2.5^2 20 (M6, tap drill 5) + pi 2.1^2 8 (M5, tap drill 4.2, flat) + pi 1.65^2 35; faces +1 +2 +1", base=C4)
C5 = up(T5, "013-part-holes")

# ---------------------------------------------------------------- 06 threads
def thr(face, **p):
    return C("solid.thread", face=face, modeled=True, **p)
def ext_volume(d, P, sections):
    """2 pi int int r m(r) over the stock's meridian section, given as [(z0, z1, rmax(z))]."""
    r1, m = iso_material_fraction_ext(d, P)
    def ring(rmax):
        a = quad(lambda r: r, 0, min(rmax, r1)) if rmax > 0 else 0.0
        b = quad(lambda r: r * m(r), r1, rmax, 64) if rmax > r1 else 0.0
        return a + b
    return 2 * PI * sum(quad(lambda z: ring(f(z)), z0, z1, 64) for z0, z1, f in sections)
TOL_T = {"rel": 5e-3}
TMETH = ("ISO 68-1 basic profile (60 deg, root flat P/4 at d - 5H/4 ... minor d1 = d - 1.0825 P, crest flat P/8): a helical groove of fixed section is a screw motion, "
         "so averaged over the turn every meridian half-plane holds the same section and V = 2 pi int int r m(r) dr dz over the stock's section, m the material fraction per pitch; "
         "the groove runs out past free ends. Tolerance 0.5%: solid.thread cuts faceted by design (chords within P/200 on the body, P/100 on the tool)")
cylT = [cyl([0, 0, 0], 5, 30, body_name="Part")]
case(T6, "001-external-m10-full", "a modelled M10 thread over the whole of a d10 x 30 cylinder", cylT + [thr([5, 0, 15], designation="M10")],
     {"body_count": 1, "volume_mm3": ext_volume(10, 1.5, [(0, 30, lambda z: 5)]), "tolerance": TOL_T},
     TMETH + "; P = 1.5, L = 30")
case(T6, "002-external-m10x1-fine", "a modelled fine M10x1 thread over a d10 x 30 cylinder", cylT + [thr([5, 0, 15], designation="M10x1")],
     {"body_count": 1, "volume_mm3": ext_volume(10, 1.0, [(0, 30, lambda z: 5)]), "tolerance": TOL_T},
     TMETH + "; P = 1, L = 30")
# A square stop inside the material takes exactly length/P turns: the groove's section is symmetric
# about its centre line, so what the sweep's last section leaves uncut ahead of the stop it cuts
# behind it, in mirror image. Its first moment does not cancel, by under 0.002 mm of the centre here.
v3 = PI * 25 * 15 + ext_volume(10, 1.5, [(15, 30, lambda z: 5)])
z3 = (PI * 25 * 15 * 7.5 + ext_volume(10, 1.5, [(15, 30, lambda z: 5)]) * 22.5) / v3
case(T6, "003-external-m10-partial-length", "a modelled M10 thread over 15 of the 30 mm from the top end (`from`): it stops square in the middle of the face",
     cylT + [thr([5, 0, 15], designation="M10", length=15, **{"from": [0, 0, 30]})],
     {"body_count": 1, "volume_mm3": v3, "center_of_mass": [0, 0, z3], "bbox": {"min": [-5, -5, 0], "max": [5, 5, 30]}, "tolerance": {"rel": 5e-4}},
     TMETH.replace("Tolerance 0.5%: solid.thread cuts faceted by design (chords within P/200 on the body, P/100 on the tool)", "") +
     "; the thread runs from the top end (z=30, out past it into the air) down to z=15, where it stops square: there the groove's section is symmetric about its centre line, "
     "so the sweep's last section cuts behind the stop what it leaves ahead of it, and the stop removes exactly as if the groove ended at z=15: "
     "V = pi 5^2 15 + 2 pi int_15^30 int r m(r) dr dz. The centre of mass is on the axis (ten whole turns; the stop's pieces are under 0.02 mm off it) at "
     "z = (pi 25 15 x 7.5 + V_threaded x 22.5) / V; the stop's first moment, O(w^2), moves it under 0.002 mm. A pick at z=15 alone would be refused: both ends are free. "
     "Tolerance 0.05%: the groove is SMLib's exact screw sweep")
shaft = [cyl([0, 0, 0], 8, 10, body_name="Part"), cyl([0, 0, 10], 5, 25, operation="join", targets=["Part"])]
# At a shoulder the groove stops with its widest part clear of it: its centre a half groove width w short
# of the shoulder, w taken at the tool's outer edge 0.05 P past the crest (the section reaches past the
# surface so the cut leaves no skin). The collar is whole.
def ext_half_width(d, P):
    H = math.sqrt(3) / 2 * P
    return (P / 4 + 2 * (d / 2 + 0.05 * P - (d / 2 - 5 * H / 8)) * math.tan(PI / 6)) / 2
w10 = ext_half_width(10, 1.5)
SHOULDER = ("the shaft's lower end is a shoulder (the d16 collar lies past it, in the groove's way), so the groove stops there with its widest part clear of it: "
            "its centre w = (P/4 + 2 (d/2 + 0.05P - r_root) tan30)/2 = %.6f mm above the shoulder (its half width at the tool's outer edge, 0.05 P past the crest), "
            "and the square stop inside material removes exactly as if the groove ended at its centre (see 003); the top end is free, the groove runs out into the air. " % w10)
vt4 = ext_volume(10, 1.5, [(10 + w10, 35, lambda z: 5)])
v4 = PI * 64 * 10 + PI * 25 * w10 + vt4
z4 = (PI * 64 * 10 * 5 + PI * 25 * w10 * (10 + w10 / 2) + vt4 * (10 + w10 + 35) / 2) / v4
case(T6, "004-external-m10-to-shoulder", "a modelled M10 thread on a d10 shaft running down to a d16 shoulder: it stops clear of it, the collar whole", shaft + [thr([5, 0, 25], designation="M10")],
     {"body_count": 1, "volume_mm3": v4, "center_of_mass": [0, 0, z4], "bbox": {"min": [-8, -8, 0], "max": [8, 8, 35]}, "tolerance": {"rel": 5e-4}},
     SHOULDER + "V = pi 8^2 10 (the collar) + pi 5^2 w + 2 pi int_{10+w}^35 int r m(r) dr dz (m as in 001); the centre of mass from the three parts' moments (the threaded part's at its "
     "middle, the screw averaging every meridian alike; its stop moves it under 0.002 mm). Tolerance 0.05%: the groove is SMLib's exact screw sweep; a groove running a pitch into the collar "
     "(the old free-end overrun) removes 0.9% more")
dI, PI_ = 10, 1.5
g = iso_groove_fraction_int(dI, PI_)
vg = 20 * 2 * PI * quad(lambda r: r * g(r), 4.25, 5)
case(T6, "005-internal-m10-through", "a modelled internal M10 thread in a d8.5 (tap drill) hole through a 30x30x20 block",
     [box([0, 0, 0], 30, 30, 20, body_name="Part"), H(position=[15, 15, 20], diameter=8.5), thr([19.25, 15, 10], designation="M10")],
     {"body_count": 1, "volume_mm3": 18000 - PI * 4.25 ** 2 * 20 - vg, "tolerance": TOL_T},
     "ISO 68-1 internal basic profile: groove P/8 wide at the major diameter, growing 2 tan30 per mm inward; the drilled hole r4.25 is above the minor r4.188, so the groove removes 2 pi int_4.25^5 r g(r) dr per mm; screw motion argument as in 001; both ends run out")
case(T6, "006-external-m10-into-chamfer", "a d10 x 30 cylinder with a 1 mm chamfer on top, then a modelled M10 thread over its cylinder face, running out through the chamfer",
     cylT + [C("solid.chamfer", edges=[[5, 0, 30]], distance=1), thr([5, 0, 15], designation="M10")],
     {"body_count": 1, "volume_mm3": ext_volume(10, 1.5, [(0, 29, lambda z: 5), (29, 30, lambda z: 5 - (z - 29))]), "tolerance": TOL_T},
     TMETH + "; the stock's section is r <= 5 below z=29 and r <= 5 - (z-29) in the chamfer; the groove runs one pitch past z=29, which covers the chamfer down to r=4.2 (above the root r=4.188 only for 0.01 mm)")
# The r1 fillet at the shoulder adds the ring of the corner square [5,6]x[10,11] less the quarter disc about
# (6, 11): 2 pi (1 x 5.5 - pi/4 (6 - 4/(3 pi))). The cylinder face then starts at z=11.
vf7 = 2 * PI * (5.5 - PI / 4 * (6 - 4 / (3 * PI)))
zf7 = 2 * PI * (5.5 * 10.5 - PI / 4 * (6 - 4 / (3 * PI)) * (11 - 4 / (3 * PI))) / vf7
vt7 = ext_volume(10, 1.5, [(11 + w10, 35, lambda z: 5)])
v7 = PI * 64 * 10 + vf7 + PI * 25 * (1 + w10) + vt7
z7 = (PI * 64 * 10 * 5 + vf7 * zf7 + PI * 25 * (1 + w10) * (10 + (1 + w10) / 2) + vt7 * (11 + w10 + 35) / 2) / v7
case(T6, "007-external-m10-at-filleted-shoulder", "a modelled M10 thread on a d10 shaft whose root at the d16 shoulder has an r1 fillet: it stops clear of the fillet",
     shaft + [C("solid.fillet", edges=[[5, 0, 10]], radius=1), thr([5, 0, 25], designation="M10")],
     {"body_count": 1, "volume_mm3": v7, "center_of_mass": [0, 0, z7], "bbox": {"min": [-8, -8, 0], "max": [8, 8, 35]}, "tolerance": {"rel": 5e-4}},
     "the fillet's ring: the corner square [5,6]x[10,11] less the quarter disc about (6,11), V_f = 2 pi (5.5 - pi/4 (6 - 4/(3 pi))), its centroid z from the same moments; "
     "the cylinder face starts at z=11, where the fillet meets it. " + SHOULDER.replace("lower end is a shoulder (the d16 collar", "lower end (z=11) is a shoulder (the fillet and the collar") +
     "V = pi 8^2 10 + V_f + pi 5^2 (1 + w) + 2 pi int_{11+w}^35 int r m(r) dr dz. Tolerance 0.05%: SMLib's exact screw sweep")
case(T6, "008-multi-start-thread", "a two-start M10 thread (lead 3, pitch 1.5) over a d10 x 30 cylinder", cylT + [thr([5, 0, 15], designation="M10", starts=2)],
     {"body_count": 1, "volume_mm3": ext_volume(10, 1.5, [(0, 30, lambda z: 5)]), "tolerance": {"rel": 1e-4}},
     TMETH.replace("Tolerance 0.5%: solid.thread cuts faceted by design (chords within P/200 on the body, P/100 on the tool)", "Tolerance 0.01%: SMLib's exact screw sweep") +
     "; two grooves of lead 2P, the second a half turn round from the first: every meridian half-plane holds the same sections a pitch apart as 001's one groove, "
     "so the material fraction and the volume are 001's")
# The runout: the section moves out of the shaft as it turns, h = d/2 - root + 0.05 P at the shoulder
# end to 0 three mm (2 P) on; the cut per radian of turn is f(o) = int r w(r - o) dr from the moved root
# out to d/2, a cubic in the offset o until the root reaches the surface (Simpson exact).
def m10_f(o, P=1.5, d=10):
    root = d / 2 - 5 * math.sqrt(3) / 16 * P
    q1 = 2 * math.tan(PI / 6)
    q0 = P / 4 - q1 * root - q1 * o
    a = root + o
    if a >= d / 2:
        return 0.0
    F = lambda r: q0 * r * r / 2 + q1 * r ** 3 / 3
    return F(d / 2) - F(a)
P10 = 1.5
root10 = 5 - 5 * math.sqrt(3) / 16 * P10
h10 = 5 - root10 + 0.05 * P10
top10 = 5 - root10
mean10 = sum((m10_f(a) + 4 * m10_f((a + b) / 2) + m10_f(b)) * (b - a) / 6 for a, b in [(top10 * i / 64, top10 * (i + 1) / 64) for i in range(64)]) / h10
s_lo = 10 + w10
v9 = PI * 64 * 10 + PI * 25 * 25 - 2 * PI * (m10_f(0) * (35 - (s_lo + 3)) / P10 + 3 / P10 * mean10)
case(T6, "009-thread-runout-length", "004's M10 thread run from the free top end, vanishing over a 3 mm (2 P) runout before the d16 shoulder",
     shaft + [thr([5, 0, 25], designation="M10", ease_out=3, **{"from": [0, 0, 35]})],
     {"body_count": 1, "volume_mm3": v9, "bbox": {"min": [-8, -8, 0], "max": [8, 8, 35]}, "tolerance": {"rel": 1e-4}},
     SHOULDER.replace("so the groove stops there with its widest part clear of it", "so the groove (its runout included) stops with its widest part clear of it") +
     "The runout (ease_out, at the end away from `from`) moves the groove's section out of the shaft as it turns, linearly from h = d/2 - root + 0.05P at s = 10 + w to 0 at s + 3: "
     "the cut per radian is f(o) = int r w(r - o) dr from root + o to 5 (w the section's width at its own radius), the runout removes 2 pi (3/P) mean(f) (Simpson, exact on the cubic), "
     "the full-depth groove from s + 3 to the free top end 2 pi f(0) (35 - s - 3)/P (the overrun past the free end as in 001). "
     "V = pi 8^2 10 + pi 5^2 25 - both. Tolerance 0.01%: SMLib's exact radial screw sweep")
case(T6, "010-part-threaded-boss", "the running part with a modelled M16 thread on its d16 boss",
     [thr([23, 15, 25], designation="M16")], {"body_count": 1},
     "the boss stands on the base (thread to a shoulder) and has an M5 hole in its top; modelled threads are faceted, so later tiers build on the part before this step and 11-cumulative-parts adds it last", base=C5, source="invariants")

# The other standard forms (crates/doc/src/thread_forms.rs, docs/threads.md): a groove of a fixed
# straight-flanked section, its tooth `flat` wide at the crest radius and widening by tan a + tan b
# per unit of depth, removes 2 pi int r w(r) dr per turn (Pappus, the screw motion).
def form_turn(P, flat, crest, a, b, r0, r1, internal=False):
    t = math.tan(math.radians(a)) + math.tan(math.radians(b))
    w = lambda r: P - flat - ((r - crest) if internal else (crest - r)) * t
    return 2 * PI * quad(lambda r: r * w(r), r0, r1, 16)
INCH = 25.4
TOL_F = {"rel": 1e-4}
FMETH = ("groove volume per turn 2 pi int r w(r) dr over the material it cuts (Pappus on the axial section; a screw motion), w(r) = P - flat - depth (tan a + tan b); "
         "six whole turns between free ends, so V = stock - 6 x that. Tolerance 0.01%: SMLib's exact screw sweep. ")
def shaft_form(name, what, form, desig, d, P, flat, a, b, r3, extra="", **p):
    L = 6 * P
    v = PI * (d / 2) ** 2 * L - 6 * form_turn(P, flat, d / 2, a, b, r3, d / 2)
    case(T6, name, what, [cyl([0, 0, 0], d / 2, L, body_name="Part"), thr([d / 2, 0, L / 2], form=form, designation=desig, **p)],
         {"body_count": 1, "volume_mm3": v, "tolerance": TOL_F}, FMETH + extra)
t15, t145 = math.tan(math.radians(15)), math.tan(math.radians(14.5))
shaft_form("011-external-tr20x4", "a modelled trapezoidal Tr20x4 thread (ISO 2904) on a d20 shaft, six turns", "trapezoidal", "Tr20x4", 20, 4,
           2 * (1 - t15), 15, 15, 10 - 2 - 0.25,
           "ISO 2904: 30 deg, H1 = P/2, crest flat P/2 (1 - tan15) = 0.366 P, external minor d3 = d - 2 (H1 + ac) = 15.5 (ac = 0.25 for P 2..5)")
shaft_form("012-external-acme-3-4-6", "a modelled 3/4-6 ACME thread (ASME B1.5, general purpose) on a 3/4 in shaft, six turns", "acme", "3/4-6 ACME", 0.75 * INCH, INCH / 6,
           INCH / 12 * (1 - t145), 14.5, 14.5, 0.375 * INCH - INCH / 12 - 0.010 * INCH,
           "ASME B1.5: 29 deg, h = P/2, crest flat 0.3707 P, external minor d - P - 0.020 in (10 threads per inch or coarser)")
t3 = math.tan(math.radians(3)) + math.tan(math.radians(30))
shaft_form("013-external-s20x4", "a modelled metric buttress S20x4 thread (DIN 513, 3/30 deg) on a d20 shaft, six turns", "buttress", "S20x4", 20, 4,
           (4 - 3 * t3) / 2, 3, 30, 10 - 0.86777 * 4,
           "DIN 513-1: 3 deg load flank, 30 deg clearance flank, H1 = 0.75 P, flats (P - 0.75 P (tan3 + tan30))/2 = 0.26384 P, external minor d3 = d - 2 x 0.86777 P = 13.058 (IS 4696-3 table)")
t7 = math.tan(math.radians(7)) + 1
P8 = INCH / 8
shaft_form("014-external-buttress-asme-1.5-8", "a modelled 1.5-8 buttress thread (ASME B1.9 basic profile, 7/45 deg) on a 1.5 in shaft, six turns", "buttress_asme", "1.5-8 BUTT",
           1.5 * INCH, P8, (P8 - 0.6 * P8 * t7) / 2, 7, 45, 0.75 * INCH - 0.6 * P8,
           "ASME B1.9 basic profile: 7 deg load flank, 45 deg clearance flank, h = 0.6 P, flats (P - 0.6 P (tan7 + tan45))/2 = 0.16316 P; the root radius and allowances are not modelled")
dT, PT = 20, 4
vnut = 6 * form_turn(PT, 2 * (1 - t15), (dT - PT) / 2, 15, 15, (dT - PT) / 2, dT / 2 + 0.25, internal=True)
case(T6, "015-internal-tr20x4-nut", "a modelled internal Tr20x4 thread in a d16 (D1 = d - P) bore through a 40x40x24 block",
     [box([-20, -20, 0], 40, 40, 24, body_name="Part"), cyl([0, 0, -1], 8, 26, body_name="Bore"), C("solid.combine", target="Part", tools=["Bore"], operation="cut"),
      thr([8, 0, 12], designation="Tr20x4")],
     {"body_count": 1, "volume_mm3": 40 * 40 * 24 - PI * 64 * 24 - vnut, "tolerance": TOL_F},
     FMETH.replace("between free ends", "through the block, both ends free") + "ISO 2904 internal: the nut's crest flat 0.366 P at D1 = d - P = 16, its root at D4 = d + 2 ac = 20.5; "
     "the groove from the bore wall r 8 out to r 10.25, w(r) = P - 0.366 P - (r - 8) 2 tan15")
case(T6, "016-external-m10-left-hand", "001's modelled M10 thread, left-handed: the same volume, turning the other way", cylT + [thr([5, 0, 15], designation="M10", hand="left")],
     {"body_count": 1, "volume_mm3": ext_volume(10, 1.5, [(0, 30, lambda z: 5)]), "tolerance": {"rel": 1e-4}},
     TMETH.replace("Tolerance 0.5%: solid.thread cuts faceted by design (chords within P/200 on the body, P/100 on the tool)", "Tolerance 0.01%: SMLib's exact screw sweep") +
     "; a mirror image of 001's groove, so the same volume")

rf10 = 5 - 5 * math.sqrt(3) / 2 * 1.5 / 8
case(T6, "017-external-m10-chamfered-tip", "001's modelled M10 thread with ISO 4753's chamfered end at both free ends: a 45 deg cone down to the minor diameter, cut after the groove",
     cylT + [thr([5, 0, 15], designation="M10", tip="chamfer")],
     {"body_count": 1, "volume_mm3": ext_volume(10, 1.5, [(0, 5 - rf10, lambda z: rf10 + z), (5 - rf10, 25 + rf10, lambda z: 5), (25 + rf10, 30, lambda z: rf10 + 30 - z)]),
      "tolerance": {"rel": 1e-4}},
     TMETH.replace("Tolerance 0.5%: solid.thread cuts faceted by design (chords within P/200 on the body, P/100 on the tool)", "Tolerance 0.01%: SMLib's exact screw sweep") +
     "; the chamfer is axisymmetric and cut after the groove, so it only lowers the stock's outer radius: r <= d_f/2 + distance from the end (45 deg), d_f the minor diameter d - 5H/4 = 8.376 "
     "(ISO 4753 chamfered end: the end's diameter at most the minor diameter)")

# ---------------------------------------------------------------- 07 fillets
F = lambda edges, r, **p: C("solid.fillet", edges=edges, radius=r, **p)
case(T7, "001-single-edge-r5", "an r5 round on the stock's top front edge", [F([[30, 0, 20]], 5)],
     {"body_count": 1, "volume_mm3": VB - SPANDREL * 25 * 60, "faces": 7}, "removed (1 - pi/4) r^2 L", base=B1)
V72 = VB - 4 * SPANDREL * 25 * 20
case(T7, "002-four-vertical-edges-r5", "r5 rounds on the four vertical edges", [F([[0, 0, 10], [60, 0, 10], [60, 40, 10], [0, 40, 10]], 5)],
     {"body_count": 1, "volume_mm3": V72, "faces": 10}, "removed 4 (1 - pi/4) r^2 20", base=B1)
c = 5 - 5 / math.sqrt(2)
case(T7, "003-tangent-chain-r3", "an r3 round all round the top of the box with r5 vertical rounds: a tangent chain of lines and arcs",
     [F([[30, 0, 20], [60, 20, 20], [30, 40, 20], [0, 20, 20], [c, c, 20], [60 - c, c, 20], [60 - c, 40 - c, 20], [c, 40 - c, 20]], 3)],
     {"body_count": 1, "volume_mm3": V72 - SPANDREL * 9 * 160 - SPANDREL * 9 * 2 * PI * (5 - XBAR * 3), "faces": 18},
     "straight runs 2*50 + 2*30 = 160 remove (1-pi/4) r^2 each mm; at each corner the round is a torus quarter: Pappus, the section (1-pi/4) r^2 with its centroid r(10-3pi)/(12-3pi) in from the wall, at radius 5 - that, over a quarter turn",
     base=up(T7, "002-four-vertical-edges-r5"))
case(T7, "004-convex-and-concave-r5", "an L block: r5 on its inner (concave) vertical edge and on an outer (convex) one",
     sketch("XY", line([[0, 0], [40, 0], [40, 10], [10, 10], [10, 40], [0, 40]], closed=True)) + [C("solid.extrude", distance=20), F([[10, 10, 10], [0, 0, 10]], 5)],
     {"body_count": 1, "volume_mm3": 700 * 20, "faces": 10},
     "the concave round adds (1-pi/4) r^2 20, the convex one removes the same: V = 700*20")
case(T7, "005-variable-r5-to-r1", "a variable round on the top front edge, r5 at x=0 to r1 at x=60",
     [F([[30, 0, 20]], 5, type="variable", radius2=1, start=[0, 0, 20])],
     {"body_count": 1, "volume_mm3": VB - SPANDREL * 60 * (25 + 5 + 1) / 3, "faces": 7},
     "linear radius, quarter-round sections (tests/oracle/77): V = abc - (1-pi/4) L (a^2 + ab + b^2)/3", base=B1)
case(T7, "006-variable-r5-to-zero", "a variable round on the top front edge, r5 at x=0 down to 0 at the x=60 vertex",
     [F([[30, 0, 20]], 5, type="variable", radius2=0, start=[0, 0, 20])],
     {"body_count": 1, "volume_mm3": VB - SPANDREL * 60 * 25 / 3, "faces": 7},
     "as 005 with b=0 (tests/oracle/78): the round closes to the corner, which stays", base=B1)
case(T7, "007-variable-r5-to-1e-3", "a variable round on the top front edge, r5 down to 1e-3 mm",
     [F([[30, 0, 20]], 5, type="variable", radius2=0.001, start=[0, 0, 20])],
     {"body_count": 1, "volume_mm3": VB - SPANDREL * 60 * (25 + 5e-3 + 1e-6) / 3, "faces": 7},
     "as 005 with b = 1e-3: the round's end is a 1e-3 mm quarter circle on the end face", base=B1)
case(T7, "008-constant-r1e-3", "a constant 1e-3 mm round on the top front edge",
     [F([[30, 0, 20]], 0.001)],
     {"body_count": 1, "volume_mm3": VB - SPANDREL * 1e-6 * 60, "faces": 7}, "removed (1-pi/4) 1e-6 60, a valid 1e-3 round face", base=B1)
corner = [[30, 40, 20], [60, 20, 20], [60, 40, 10]]
case(T7, "009-corner-three-equal-r5", "r5 rounds on the three edges meeting at the corner (60,40,20): a sphere corner",
     [F(corner, 5)],
     {"body_count": 1, "volume_mm3": VB - SPANDREL * 25 * (60 + 40 + 20 - 15) - 125 * (1 - PI / 6), "faces": 10},
     "each edge zone outside the corner cube removes (1-pi/4) r^2 (L - r); the corner cube r^3 keeps a sphere octant pi r^3/6", base=B1)
case(T7, "010-corner-unequal-r7-r3", "an r7 vertical round at a corner, then r3 along a top edge, which runs on round the r7 arc to the other top edge",
     [F([corner[2]], 7), F([[60, 15, 20]], 3)], {"body_count": 1},
     "two radii meeting at a corner over a tangent chain; one valid body", base=B1, source="invariants")
case(T7, "016-corner-three-sequential-r5-r3-r7", "rounds r5 and r3 on the two top edges at a corner, then r7 on the vertical edge under them",
     [F([corner[0]], 5), F([corner[1]], 3, tangent_chain=False), F([[60, 40, 8]], 7)], {"body_count": 1},
     "three unequal radii at one corner, one after another; the vertical edge stays sharp below the two top rounds (x=60, y=40, z from 0 to 15), so it can be picked at z=8; one valid body", base=B1, source="invariants")
case(T7, "011-chain-ending-at-corner", "r5 rounds on two top edges meeting at a corner whose vertical edge stays sharp",
     [F([corner[0], corner[1]], 5)], {"body_count": 1},
     "two rounds ending at a sharp vertical edge; one valid body", base=B1, source="invariants")
case(T7, "012-variable-into-rounded-corner", "a vertical r5 round, then a variable round r1 -> r5 along the top back edge into it",
     [F([corner[2]], 5), F([[30, 40, 20]], 1, type="variable", radius2=5, start=[0, 40, 20])], {"body_count": 1},
     "a variable round running into a constant one at a corner; one valid body", base=B1, source="invariants")
missing(T7, "013-corner-setback", "three r5 rounds at a corner with setbacks of 8 mm along each edge", "solid.fillet has no corner setback distances",
        "the setback corner patch would be graded by invariants")
case(T7, "014-curved-edge-r2", "an r2 round on the top edge of a cylinder r10 h20",
     [cyl([0, 0, 0], 10, 20, body_name="Part"), F([[10, 0, 20]], 2)],
     {"body_count": 1, "volume_mm3": PI * 100 * 20 - SPANDREL * 4 * 2 * PI * (10 - XBAR * 2), "faces": 4},
     "a torus round: Pappus on the removed section (1-pi/4) r^2 at radius 10 - r(10-3pi)/(12-3pi)")
case(T7, "015-corner-g2-patch", "three r5 rounds at a corner with the g2 corner patch",
     [F(corner, 5, corner="g2")], {"body_count": 1, "faces": 10},
     "the g2 patch replaces the sphere octant; no closed-form volume; one valid body, the same faces as 009", base=B1, source="invariants")
V7 = V5 - 2 * SPANDREL * 25 * 20 - SPANDREL * 9 * 60 + SPANDREL * 1 * 2 * PI * (8 + XBAR * 1)
case(T7, "017-part-rounds", "the running part: r5 on the front vertical edges, r3 on the back wall's top front edge, r1 where the boss meets the top",
     [F([[0, 0, 10], [60, 0, 10]], 5), F([[30, 30, 35]], 3), F([[23, 15, 20]], 1)],
     {"body_count": 1, "volume_mm3": V7, "faces": 24},
     "removed 2 (1-pi/4) 25 20 + (1-pi/4) 9 60; the concave r1 at the boss adds (1-pi/4) 1 2 pi (8 + (10-3pi)/(12-3pi)); none of them reaches a hole, the slot or the window", base=C5)
C7 = up(T7, "017-part-rounds")

# ---------------------------------------------------------------- 08 chamfers
K = lambda edges, d, **p: C("solid.chamfer", edges=edges, distance=d, **p)
case(T8, "001-equal-5", "a 5 mm chamfer on the top front edge", [K([[30, 0, 20]], 5)],
     {"body_count": 1, "volume_mm3": VB - 12.5 * 60, "faces": 7}, "removed d^2/2 L", base=B1)
case(T8, "002-two-distances-5-3", "a 5 x 3 chamfer on the top front edge", [K([[30, 0, 20]], 5, distance2=3)],
     {"body_count": 1, "volume_mm3": VB - 7.5 * 60, "faces": 7}, "removed d1 d2 / 2 L", base=B1)
case(T8, "003-distance-angle-5-30", "a chamfer 5 mm at 30 deg on the top front edge", [K([[30, 0, 20]], 5, angle=30)],
     {"body_count": 1, "volume_mm3": VB - 0.5 * 5 * 5 * math.tan(math.radians(30)) * 60, "faces": 7}, "legs 5 and 5 tan30: removed 5 * 5 tan30 / 2 * 60", base=B1)
d = 3
case(T8, "004-corner-three-equal-3", "3 mm chamfers on the three edges meeting at a corner",
     [K(corner, 3)],
     {"body_count": 1, "volume_mm3": VB - (d * d / 2 * 120 - d ** 3 + d ** 3 / 4), "faces": 9},
     "the union of the three prisms {b+c<=d}, {a+c<=d}, {a+b<=d} (a, b, c distances from the corner faces): sum d^2/2 L - 3 d^3/3 + the triple part d^3/4 (exact integral)", base=B1)
case(T8, "005-corner-two-mitred-3", "3 mm chamfers on two top edges meeting at a corner (mitred)",
     [K(corner[:2], 3)],
     {"body_count": 1, "volume_mm3": VB - (d * d / 2 * 100 - d ** 3 / 3), "faces": 8},
     "two prisms meeting in a mitre: d^2/2 (60 + 40) - d^3/3", base=B1)
case(T8, "006-curved-edge-2", "a 2 mm chamfer on the top edge of a cylinder r10 h20",
     [cyl([0, 0, 0], 10, 20, body_name="Part"), K([[10, 0, 20]], 2)],
     {"body_count": 1, "volume_mm3": PI * 100 * 20 - 2 * 2 * PI * (10 - 2 / 3), "faces": 4},
     "a cone chamfer: Pappus on the triangle (area 2) with its centroid at radius 10 - 2/3")
case(T8, "007-chamfer-then-fillets", "a 3 mm chamfer on the top front edge, then r2 rounds on the front vertical edges that meet it",
     [K([[30, 0, 20]], 3), F([[0, 0, 5], [60, 0, 5]], 2)], {"body_count": 1},
     "rounds running into a chamfer face; one valid body", base=B1, source="invariants")
V8 = V7 - 0.5 * 2 * PI * (8 - 1 / 3) - 2 * 60
case(T8, "008-part-chamfers", "the running part: 1 mm on the boss top edge, 2 mm on the back wall's top back edge",
     [K([[23, 15, 30]], 1), K([[30, 40, 35]], 2)],
     {"body_count": 1, "volume_mm3": V8, "faces": 26},
     "removed 0.5 * 2 pi (8 - 1/3) (Pappus) + 2 * 60", base=C7)
C8 = up(T8, "008-part-chamfers")

# ---------------------------------------------------------------- 09 shells and offsets
S = lambda faces, t, **p: C("solid.shell", faces=faces, thickness=t, **p)
case(T9, "001-box-open-top-t2", "the stock shelled 2 mm, open at the top", [S([[30, 20, 20]], 2)],
     {"body_count": 1, "volume_mm3": VB - 56 * 36 * 18, "faces": 11}, "cavity 56x36x18; faces 5 outer + rim + 5 inner", base=B1)
case(T9, "002-cylinder-open-top-t1", "a cylinder r10 h30 shelled 1 mm, open at the top",
     [cyl([0, 0, 0], 10, 30, body_name="Part"), S([[0, 0, 30]], 1)],
     {"body_count": 1, "volume_mm3": PI * 100 * 30 - PI * 81 * 29, "faces": 5}, "cavity r9 x 29")
case(T9, "003-tube-two-open-faces", "the stock shelled 2 mm, open top and bottom", [S([[30, 20, 20], [30, 20, 0]], 2)],
     {"body_count": 1, "volume_mm3": 20 * (2400 - 56 * 36), "faces": 10}, "a rectangular tube: 20 (60*40 - 56*36)", base=B1)
case(T9, "004-thin-wall-0.2", "the stock shelled 0.2 mm, open at the top", [S([[30, 20, 20]], 0.2)],
     {"body_count": 1, "volume_mm3": VB - 59.6 * 39.6 * 19.8, "faces": 11}, "cavity 59.6 x 39.6 x 19.8", base=B1)
case(T9, "005-shell-rounds-r5-t2", "the box with r5 vertical rounds shelled 2 mm open at the top: the inner corners r3",
     [S([[30, 20, 20]], 2)],
     {"body_count": 1, "volume_mm3": V72 - 18 * (56 * 36 - 4 * SPANDREL * 9), "faces": 19},
     "the cavity is the 56x36 rounded rectangle with r 5-2 = 3 corners, 18 deep", base=up(T7, "002-four-vertical-edges-r5"))
case(T9, "006-shell-rounds-r1-thinner-than-t2", "the box with r1 vertical rounds shelled 2 mm: the inner corners close sharp",
     [F([[0, 0, 10], [60, 0, 10], [60, 40, 10], [0, 40, 10]], 1), S([[30, 20, 20]], 2)],
     {"body_count": 1, "volume_mm3": 20 * (2400 - 4 * SPANDREL) - 18 * 56 * 36, "faces": 15},
     "an inward offset of r1 by 2 vanishes: the cavity is a sharp 56x36x18 box", base=B1)
case(T9, "007-offset-top-face-5", "the stock's top face offset 5 mm", [C("solid.offset_face", faces=[[30, 20, 20]], distance=5)],
     {"body_count": 1, "volume_mm3": 60 * 40 * 25, "faces": 6}, "a 60x40x25 box", base=B1)
case(T9, "008-offset-cylinder-wall-2", "a cylinder r10 h20 with its side face offset 2 mm",
     [cyl([0, 0, 0], 10, 20, body_name="Part"), C("solid.offset_face", faces=[[10, 0, 10]], distance=2)],
     {"body_count": 1, "volume_mm3": PI * 144 * 20, "faces": 3}, "a cylinder r12 h20")
case(T9, "009-shell-outside-t2", "the stock shelled 2 mm outward, open at the top", [S([[30, 20, 20]], 2, direction="outside")],
     {"body_count": 1, "volume_mm3": 64 * 44 * 22 - VB, "faces": 11}, "walls grown out of the closed faces: 64 x 44 x 22 less the original box", base=B1)
case(T9, "010-shell-under-rounded-top", "the stock with r3 rounds on its four top edges, shelled 2 mm open at the bottom",
     [F([[30, 0, 20], [60, 20, 20], [30, 40, 20], [0, 20, 20]], 3), S([[30, 20, 0]], 2)], {"body_count": 1},
     "walls meeting rounds and their corner patches; one valid body", base=B1, source="invariants")
case(T9, "011-part-shell", "the running part shelled 1.5 mm open at the bottom",
     [S([[30, 20, 0]], 1.5)], {"body_count": 1},
     "a shell around holes, a slot, a window, rounds and chamfers; one valid body", base=C8, source="invariants")
C9 = up(T9, "011-part-shell")
case(T9, "012-closed-shell-sealed-void", "the stock shelled 2 mm with no face open: a closed body with a sealed void", [S([], 2)],
     {"body_count": 1, "shells": 2, "volume_mm3": VB - 56 * 36 * 16, "faces": 12},
     "void 56x36x16: V = 48000 - 32256; faces 6 outer + 6 inner; one body, an outer and an inner shell", base=B1)
DOME = sketch(vplane([0, 0, 0]), line([[0, 0], [20, 0]]), C("sketch.arc.center_point", center=[0, 0], start=[20, 0], end=[0, 20]), line([[0, 20], [0, 0]]))
case(T9, "013-dome-shelled-from-base", "a dome r20 shelled 2 mm, open at its flat base: the rim is the circle r18 where the inner sphere meets the base plane",
     DOME + [C("solid.revolve", axis="Z", angle="360 deg", operation="new"), S([[10, 0, 0]], 2)],
     {"body_count": 1, "volume_mm3": 2 / 3 * PI * (20 ** 3 - 18 ** 3), "faces": 3},
     "V = (2/3) pi (20^3 - 18^3); faces: outer sphere, inner sphere, base ring")
dr5 = 20 * math.tan(math.radians(5))
case(T9, "014-drafted-box-with-rounds-shelled",
     "a 120x80x40 box drafted 5 degrees with 4 mm rounds on the sloping corners, shelled 2 mm open at the top: the inner rounds' edges must lie on their faces (#3573 reports the same design)",
     sketch("XY", C("sketch.rectangle.center", center=[0, 0], corner=[60, 40]))
     + [C("solid.extrude", distance=40, taper="-5 deg", operation="new"),
        F([[sx * (60 - dr5), sy * (40 - dr5), 20] for sx, sy in [(1, 1), (1, -1), (-1, 1), (-1, -1)]], 4), S([[0, 0, 40]], 2)],
     {"body_count": 1},
     "no closed-form volume (tilted rounds). Invariants: the shell succeeds, one closed body, and the record's geometry check holds "
     "(every edge of the four inner rounds lies on its faces; the old offset left them up to 0.02 mm off, which other STEP readers reject)",
     source="invariants", sources=["tangency-labs/solidcraft#3576", "tangency-labs/solidcraft#3573"])
case(T9, "015-shelled-dome-ring-joined-back",
     "a dome (a quarter disc r20 revolved) shelled 2 mm from its flat base, then the base ring (r 18..20) face-extruded 10 mm back into the dome as a join: ~120 s, then 'boolean Union: no result'",
     sketch(vplane([0, 0, 0]), line([[0, 0], [20, 0]]), C("sketch.arc.three_point", start=[20, 0], through=[14.142136, 14.142136], end=[0, 20]), line([[0, 20], [0, 0]]), name="Quarter")
     + [C("solid.revolve", sketch="Quarter", axis="y", body_name="Dome"), S([[0, 0, 0]], 2),
        C("solid.extrude", face=[19, 0, 0], face_body="Dome", distance=10, direction="negative", operation="join")],
     {"body_count": 1, "volume_mm3": 2 / 3 * PI * (20 ** 3 - 18 ** 3) + 760 * PI - 2 * PI / 3 * 76 ** 1.5},
     "shell: (2/3) pi (20^3 - 18^3). The ring's prism r 18..20, z 0..10 lies inside the dome's wall where r^2 + z^2 <= 20^2 (r >= 18 keeps it outside the cavity) and pokes out elsewhere: "
     "it adds 760 pi - Int_18^20 2 pi r sqrt(400 - r^2) dr = 760 pi - (2 pi/3) 76^1.5",
     sources=["tangency-labs/solidcraft#4847"])

# ---------------------------------------------------------------- 10 patterns and mirrors
case(T10, "001-rect-pattern-holes", "a d5 hole patterned 3 x 2 at 20 mm in a 60x40x10 plate",
     [box([0, 0, 0], 60, 40, 10, body_name="Part"), H(position=[10, 10, 10], diameter=5, name="H"),
      C("solid.pattern.rectangular", features=["H"], dir1=[1, 0, 0], count1=3, spacing1=20, dir2=[0, 1, 0], count2=2, spacing2=20)],
     {"body_count": 1, "volume_mm3": 24000 - 6 * PI * 6.25 * 10, "faces": 12}, "six holes pi 2.5^2 10")
case(T10, "002-circular-pattern-holes", "a d6 hole patterned 8 times round a disk r30 h10",
     [cyl([0, 0, 0], 30, 10, body_name="Part"), H(position=[20, 0, 10], diameter=6, name="H"), C("solid.pattern.circular", features=["H"], axis="Z", count=8)],
     {"body_count": 1, "volume_mm3": PI * 900 * 10 - 8 * PI * 9 * 10, "faces": 11}, "eight holes pi 3^2 10")
# union of 6 disks r5 centred on a circle r6: polar integral of the covered radial intervals
def covered(theta):
    iv = []
    for k in range(6):
        a = theta - k * PI / 3
        disc = 36 * math.cos(a) ** 2 - 11
        if disc > 0 and math.cos(a) > 0:
            iv.append((6 * math.cos(a) - math.sqrt(disc), 6 * math.cos(a) + math.sqrt(disc)))
    iv.sort()
    tot, cur = 0.0, None
    for a, b in iv:
        if cur is None or a > cur[1]:
            if cur: tot += (cur[1] ** 2 - cur[0] ** 2) / 2
            cur = [a, b]
        else:
            cur[1] = max(cur[1], b)
    return tot + ((cur[1] ** 2 - cur[0] ** 2) / 2 if cur else 0)
Au = 12 * quad_split(covered, 0, PI / 6, 400, 32)
Au_check = 6 * PI * 25  # sanity: less than the disks' sum
assert Au < Au_check
case(T10, "003-overlapping-pattern-leaves-core", "six r5 cuts patterned round a circle r6 in a disk r20: they overlap and cut a core free",
     [cyl([0, 0, 0], 20, 10, body_name="Part"), cyl([6, 0, -1], 5, 12, operation="cut", targets=["Part"], name="H"), C("solid.pattern.circular", features=["H"], axis="Z", count=6)],
     {"body_count": 2, "volume_mm3": PI * 400 * 10 - Au * 10},
     f"neighbouring cuts overlap (centres 6 apart < 10) but none reaches the axis (6 > 5): a core is cut free; the union's area {Au:.9f} by a polar integral of the covered radial intervals")
case(T10, "004-mirror-body-joined", "a 30x40x20 half with a d10 hole mirrored about x=30 and joined: the faces at x=30 coincide",
     [box([0, 0, 0], 30, 40, 20, body_name="Part"), H(position=[20, 20, 20], diameter=10),
      C("solid.mirror", bodies=["Part"], plane={"origin": [30, 0, 0], "normal": [1, 0, 0]}, combine=True)],
     {"body_count": 1, "volume_mm3": VB - 2 * PI * 25 * 20, "faces": 8}, "the stock with two holes at x=20 and 40; the mirror face merges away")
case(T10, "005-mirror-feature", "a d10 hole in the stock mirrored about x=30 as a feature",
     [H(position=[20, 20, 20], diameter=10, name="H"), C("solid.mirror", features=["H"], plane={"origin": [30, 0, 0], "normal": [1, 0, 0]})],
     {"body_count": 1, "volume_mm3": VB - 2 * PI * 25 * 20, "faces": 8}, "two holes pi 5^2 20", base=B1)
case(T10, "006-circular-pattern-counterbores", "a counterbored hole (d5, d9 x 3) patterned 6 times round a disk r30 h10",
     [cyl([0, 0, 0], 30, 10, body_name="Part"), H(position=[20, 0, 10], diameter=5, type="counterbore", cb_diameter=9, cb_depth=3, name="CB"),
      C("solid.pattern.circular", features=["CB"], axis="Z", count=6)],
     {"body_count": 1, "volume_mm3": PI * 900 * 10 - 6 * (PI * 6.25 * 7 + PI * 20.25 * 3), "faces": 21}, "each removes pi 2.5^2 7 + pi 4.5^2 3; 3 faces each")
case(T10, "007-pattern-bodies", "a pin r3 h10 patterned 4 times at 10 mm as bodies",
     [cyl([0, 0, 0], 3, 10, body_name="Pin"), C("solid.pattern.rectangular", bodies=["Pin"], dir1=[1, 0, 0], count1=4, spacing1=10)],
     {"body_count": 4, "volume_mm3": 4 * PI * 9 * 10, "faces": 12}, "four separate pins")
case(T10, "008-part-vents", "the running part with a d3 vent hole in its top patterned 4 times at 5 mm",
     [H(position=[30, 24, 20], diameter=3, name="Vent"), C("solid.pattern.rectangular", features=["Vent"], dir1=[1, 0, 0], count1=4, spacing1=5)],
     {"body_count": 1}, "holes through the shelled top skin; one valid body", base=C9, source="invariants")
C10 = up(T10, "008-part-vents")

# ---------------------------------------------------------------- 11 cumulative parts
VL = 40 * 816 + SPANDREL * 25 * 40 - 12.5 * 40 - 2 * PI * 16 * 8
case(T11, "001-bracket", "an L bracket: concave r5 round, 5 mm outer chamfer, a hole in each leg",
     sketch(plane([0, 40, 0], [1, 0, 0], [0, 0, 1]), line([[0, 0], [60, 0], [60, 8], [8, 8], [8, 50], [0, 50]], closed=True)) + [C("solid.extrude", distance=40, body_name="Part"),
      F([[8, 20, 8]], 5), K([[0, 20, 0]], 5), H(position=[40, 20, 8], diameter=8), H(position=[8, 20, 30], diameter=8)],
     {"body_count": 1, "volume_mm3": VL, "faces": 12},
     "L section 60*8 + 8*42 = 816 times 40; + (1-pi/4) 25 40 (concave round) - 12.5*40 (chamfer) - 2 pi 4^2 8 (holes, 8 thick legs); features apart from each other")
Vf = PI * ((1600 - 64) * 10 + (225 - 64) * 20) - 6 * PI * 16 * 10 - 1.125 * 2 * PI * 14.5 + SPANDREL * 9 * 2 * PI * (15 + 3 * XBAR)
FL = sketch(vplane([0, 0, 0]), line([[8, 0], [40, 0], [40, 10], [15, 10], [15, 30], [8, 30]], closed=True)) + [C("solid.revolve", axis="Z", body_name="Part"),
      H(position=[28, 0, 10], diameter=8, name="Bolt"), C("solid.pattern.circular", features=["Bolt"], axis="Z", count=6),
      K([[15, 0, 30]], 1.5), F([[15, 0, 10]], 3)]
case(T11, "002-flange", "a revolved flange and hub with a bore, six bolt holes, a hub chamfer and a hub-root round", FL,
     {"body_count": 1, "volume_mm3": Vf, "faces": 14},
     "pi((40^2-8^2)10 + (15^2-8^2)20) - 6 pi 4^2 10 - 1.125 * 2 pi 14.5 (chamfer, Pappus) + (1-pi/4) 9 * 2 pi (15 + 3(10-3pi)/(12-3pi)) (concave round)")
case(T11, "003-threaded-flange", "the flange with a modelled M30 thread on its hub, between the round and the chamfer, and M8 tapped holes in the disk",
     [thr([15, 0, 20], designation="M30"), H(position=[0, 28, 10], thread="M8", name="Tap"), C("solid.pattern.circular", features=["Tap"], axis="Z", count=3)],
     {"body_count": 1}, "a thread meeting a round and a chamfer, tapped holes; one valid body", base=up(T11, "002-flange"), source="invariants")
case(T11, "004-housing", "the running part with every tier: back wall, boss, slot, spline window, tapped holes, rounds, chamfers, shell, vents, and a modelled M16 thread on the boss",
     [thr([23, 15, 25], designation="M16")], {"body_count": 1},
     "the last case carries every tier; one valid body", base=C10, source="invariants")

# manifest: tiers in order, cases in authoring order until --record sorts them by time
WHAT = {
    T1: "primitives with closed-form volumes", T2: "booleans in the degeneracy classes, and cuts that end in a single line",
    T3: "extrudes and revolves of line-and-arc profiles", T4: "spline profiles, sweeps and lofts", T5: "holes and tapped holes",
    T6: "modelled threads", T7: "constant, variable and corner fillets", T8: "chamfers", T9: "shells and offsets",
    T10: "patterns and mirrors", T11: "parts combining every tier"}
mpath = os.path.join(ROOT, "ladder.json")
order = [T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11]
try:
    with open(mpath) as f:
        listed = {t["tier"]: t["cases"] for t in json.load(f)["tiers"]}
except FileNotFoundError:
    listed = {}
tiers = []
for t in order:
    made = [f"ladder/{t}/{n}" for tt, n in CASES if tt == t]
    # The recorded order first, then cases new to the tier in the order they are made here.
    kept = [c for c in listed.get(t, []) if c in made or not c.startswith("ladder/")]
    tiers.append({"tier": t, "what": WHAT[t], "cases": kept + [c for c in made if c not in kept]})
m = {"what": "the kernel complexity ladder: tiers in order of complexity, each building on the shapes before it; cases within a tier in order of measured time (cargo xtask kernel-cases --ladder --record)",
     "tiers": tiers}
with open(mpath, "w") as f:
    json.dump(m, f, indent=2, sort_keys=True)
    f.write("\n")
print(len(CASES), "cases")
