#!/usr/bin/env python3
"""Render the three class-A surfacing pictures of docs/screenshots with the running app.

    solidcraft --control 7878 &
    python3 examples/demo/class_a_shots.py --port 7878 OUTDIR

Each picture is built from commands on a new design and drawn by the control channel's
`ui.render` (1280 x 800, the window's theme, no origin axes):
  class-a-blend.png     a 24 mm box and a 10 mm box, 20 mm apart, bridged by `surface.blend` (G1 at both edges);
  class-a-boundary.png  `surface.boundary` through two 3D sketch splines;
  class-a-crown.png     `surface.patch crown=0.3` over the four straight edges of a skew rim
                        (the rim's own surface is hidden).
"""
import argparse
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from enclosure_mcp import control  # noqa: E402


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("outdir")
    ap.add_argument("--port", type=int, default=7878)
    a = ap.parse_args()

    def ui(method, params=None):
        r = control(a.port, method, params)
        if not r.get("ok", False):
            raise RuntimeError(f"{method} {params}: {r.get('error')}")
        return r.get("result")

    def ex(command, params):
        return ui("engine.execute", {"command": command, "params": params})

    def fresh():
        ui("ui.documents", {"action": "new"})
        time.sleep(1)
        ui("ui.home", {"open": False})
        ui("ui.set", {"showOrigin": False, "showSketches": False})

    def shoot(name):
        ui("ui.view", {"view": "iso"})
        ui("ui.camera", {"fit": True})
        time.sleep(1.5)
        ui("ui.render", {"path": os.path.join(os.path.abspath(a.outdir), name), "width": 1280, "height": 800})

    ui("ui.prefs", {"theme": "dark", "button": "apply"})
    ui("ui.resize", {"width": 1920, "height": 1080})

    fresh()
    ex("solid.box", {"length": 40, "width": 40, "height": 24, "corner": [0, 0, 0], "body_name": "A"})
    ex("solid.box", {"length": 40, "width": 40, "height": 10, "corner": [60, 0, 0], "body_name": "B"})
    ex("surface.blend", {"from": {"body": "A", "edges": [[40, 20, 24]]}, "to": {"body": "B", "edges": [[60, 20, 10]]}, "continuity": ["g1", "g1"], "body_name": "Blend"})
    shoot("class-a-blend.png")

    fresh()
    ex("sketch.create", {"plane": "XY", "name": "Rails"})
    ex("sketch.options", {"sketch_3d": True})
    u = [ex("sketch.spline3d", {"points": p})["curves"][0] for p in ([[0, 0, 0], [20, 0, 14], [40, 0, 0]], [[0, 30, 8], [20, 30, 0], [40, 30, 8]])]
    ex("sketch.finish", {})
    ex("surface.boundary", {"sketch": "Rails", "u": u, "body_name": "Patch"})
    shoot("class-a-boundary.png")

    fresh()
    ex("sketch.create", {"plane": "XY", "name": "Rim"})
    ex("sketch.options", {"sketch_3d": True})
    p = [[0, 0, 0], [40, 0, 8], [40, 40, 0], [0, 40, 8]]
    w = [ex("sketch.line3d", {"points": [p[i], p[j]]})["curves"][0] for i, j in ((0, 1), (0, 3), (3, 2), (1, 2))]
    ex("sketch.finish", {})
    ex("surface.boundary", {"sketch": "Rim", "u": [w[0], w[2]], "v": [w[1], w[3]], "body_name": "Rim surface"})

    def mid(i, j):
        return [(p[i][k] + p[j][k]) / 2 for k in range(3)]

    ex("surface.patch", {"body": "Rim surface", "edges": [mid(0, 1), mid(0, 3), mid(3, 2), mid(1, 2)], "crown": 0.3, "body_name": "Crown"})
    ui("ui.set", {"hiddenBodies": ["Rim surface"]})
    shoot("class-a-crown.png")


if __name__ == "__main__":
    main()
