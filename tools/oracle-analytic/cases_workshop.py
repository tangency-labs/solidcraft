#!/usr/bin/env python3
"""Analytic expected-value writer for the W23 workshop reference plates.

Run from the repository root:
    python3 tools/oracle-analytic/cases_workshop.py

All dimensions are taken from the actual parameterized oracle recipes. The
mass properties come from exact box/cylinder subtraction, rounded rectangle
quarter-spandrels and the same tangent-loop chamfer integration as case 33.
No SolidCraft result is used to create any expected geometry value.
"""

import json
import math
from pathlib import Path

from cases_features import MP, PI, box, cyl, spandrel_area, write


ROOT = Path(__file__).resolve().parents[2] / "tests" / "oracle"
CASES = (
    "84-plate-two-bores",
    "85-plate-width-100",
    "86-plate-first-bore-12",
    "87-plate-bore1-deleted",
    "88-plate-four-bores",
    "94-plate-four-rounded-corners",
    "95-plate-rounded-top-chamfer",
)


def one(case):
    recipe = json.loads((ROOT / case / "recipe.json").read_text())
    params = {
        p["name"]: float(p["expression"].removesuffix(" mm"))
        for p in recipe["parameters"]
    }
    length, width, thickness = (params[n] for n in ("length", "width", "thickness"))
    bores = [
        (f["name"], float(f["position"][0]), float(f["position"][1]),
         params[f["diameter"]])
        for f in recipe["features"] if f["op"] == "hole"
    ]
    rounded = next((f for f in recipe["features"] if f["op"] == "fillet"), None)
    chamfer = next((f for f in recipe["features"] if f["op"] == "chamfer"), None)
    perimeter = 2 * (length + width)
    top_area = length * width
    solid = box(0, 0, 0, length, width, thickness)

    if rounded is not None:
        # Four R10 vertical edges remove the same quarter-square-minus-circle
        # cross-section from each corner. Symmetry locates their combined COM.
        r = float(rounded["radius"])
        assert 0 < r < min(length, width) / 2
        removed_xy = 4 * spandrel_area(r)
        solid -= MP.at(removed_xy * thickness,
                       [length / 2, width / 2, thickness / 2])
        top_area -= removed_xy
        perimeter -= 8 * r - 2 * PI * r

    if chamfer is not None:
        # Constant-distance top chamfer around the entire rounded perimeter.
        # At depth y below the top, the removed band has area
        # P*(c-y) - pi*(c-y)^2: the inset perimeter shortens with depth.
        # Integrate this band for exact removed volume and Z first moment.
        c = float(chamfer["distance"])
        assert rounded is not None and 0 < c < rounded["radius"]
        removed = perimeter * c ** 2 / 2 - PI * c ** 3 / 3
        mean_depth = (perimeter * c ** 3 / 6 - PI * c ** 4 / 12) / removed
        solid -= MP.at(removed, [length / 2, width / 2,
                                 thickness - mean_depth])
        area = (2 * top_area + perimeter * thickness
                - c * perimeter + PI * c * c
                - c * perimeter + math.sqrt(2) * c * (perimeter - PI * c))
    else:
        area = 2 * top_area + perimeter * thickness

    holes_desc = []
    for name, x, y, diameter in bores:
        radius = diameter / 2
        solid -= cyl(x, y, radius, 0, thickness)
        area += PI * diameter * thickness - 2 * PI * radius * radius
        holes_desc.append(f"{name} at ({x:g},{y:g}) Ø{diameter:g}")

    detail = ("rectangular" if rounded is None else
              f"four exterior vertical R{rounded['radius']:g} corner rounds")
    if chamfer:
        detail += f"; full upper tangent loop {chamfer['distance']:g} mm chamfer"
    method = (f"Plate L={length:g}, W={width:g}, T={thickness:g} mm; "
              f"{'; '.join(holes_desc)}. Box minus independently located "
              "through-cylinders: V=LWT-sum(pi*d²*T/4), "
              "area=2(LW+LT+WT)+sum(pi*d*T-pi*d²/2), "
              "mass moments from exact box minus cylinders. Shape: " + detail)
    if rounded:
        method += ("; four corner spandrels subtract volume "
                   "T*(4-pi)*R² at symmetric centroid (L/2,W/2,T/2); "
                   "rounded perimeter P=2(L+W)-8R+2pi*R")
    if chamfer:
        method += ("; upper tangent chamfer subtracts volume "
                   "Vc=P*c²/2-pi*c³/3 at "
                   "(L/2,W/2,T-(P*c³/6-pi*c⁴/12)/Vc); "
                   "area from upper inset face, side strips and "
                   "sqrt(2)*c*(P-pi*c) bevel surface")
    # Every curved corner is one new vertical side face, in addition to
    # four remaining straight sides, two planar caps and independent bores.
    # A complete tangent-chain top chamfer contributes eight more faces:
    # one over each of the four straight runs and four R10 corner arcs.
    faces = 6 + len(bores) + (4 if rounded else 0) + (8 if chamfer else 0)
    write(case, method, 1, solid, area,
          [(0, 0, 0), (length, width, thickness)], faces=faces)


if __name__ == "__main__":
    for fixture in CASES:
        one(fixture)
