#!/usr/bin/env python3
"""Demo v2/v3: build an electronics enclosure in a running SolidCraft the way a person would,
with the parametric steps sent over MCP.

--story pi (default, the 0.8 README demo): a case for a Raspberry Pi 4, from a sketch dimensioned
with the board's parameters to standoffs patterned on its mounting holes, the rim rounded in one
pick, port cut-outs, vent slots, "Pi 4" engraved in DejaVu Sans Bold, a PETG material with its
mass in Properties and a height change in the Parameters dialog. Camera moves orbit about the
model under the pointer.
--story pi-0.6: the 0.6 README demo, with a colour instead of the rim, text and material.
--story v3: one standoff placed from the parameters and patterned 2 x 2 in the
Rectangular Pattern dialog, then width and wall changed in the Parameters dialog.
--story v2: four standoffs from one sketch, then the width changed over MCP.

    solidcraft --control 7878 &
    python3 examples/demo/enclosure_ui_mcp.py --port 7878 --cli solidcraft-cli [--story v2|v3]

Modelling steps go through the app's UI over its control channel: the pointer glides to a
toolbar button and clicks it (the real dialog opens), glides to the face or point it picks and
clicks, and values are typed into the dialog, as by hand. The port and boss sketches attach
directly to the enclosure's side face and inner floor, as in the original demo.
The camera follows like a user's would (Look At on a new sketch, back out to a 3/4 view on
Finish Sketch, a little orbit between features). The parameters and the dimensions that use them
are MCP `execute` calls through `solidcraft-cli mcp --connect` (enclosure_mcp.py's client).

--log writes one JSON line per caption and per pointer event, which record.sh turns into burnt-in
captions and a drawn cursor (the app gets synthetic pointer events, so the screen's own pointer
never moves).
"""

import argparse
import json
import math
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from enclosure_mcp import Mcp, control  # noqa: E402


# Seconds a caption stays up at least.
MIN_CAPTION = 1.4


class Demo:
    def __init__(self, port, mcp, log, speed, minimum_caption=MIN_CAPTION):
        self.port, self.mcp, self.log, self.speed = port, mcp, log, speed
        self.minimum_caption = minimum_caption
        self.pos = None
        self.stop = 10**6

    # -- logging -------------------------------------------------------------------------------
    def mark(self, **k):
        if self.log:
            self.log.write(json.dumps({"t": time.time(), **k}) + "\n")
            self.log.flush()

    def caption(self, text, via=""):
        self.n = getattr(self, "n", 0) + 1
        if self.n > self.stop:
            raise StopIteration
        # Every caption stays up long enough to read.
        shown = time.time() - getattr(self, "shown_at", 0.0)
        if shown < self.minimum_caption:
            time.sleep(self.minimum_caption - shown)
        self.shown_at = time.time()
        print(f"  {text}  [{via}]", file=sys.stderr)
        self.mark(caption=text, via=via)

    def wait(self, s):
        time.sleep(s * self.speed)

    # -- the app -------------------------------------------------------------------------------
    def ui(self, method, params=None):
        r = control(self.port, method, params)
        if not r.get("ok", False):
            raise RuntimeError(f"{method} {params}: {r.get('error')}")
        return r.get("result")

    def at(self, **where):
        # A clicked combo publishes its popup handles on the next rendered frame.
        # Observe that bounded transition; never substitute a command for the click.
        for attempt in range(50 if "handle" in where else 1):
            r = control(self.port, "ui.at", where)
            if r.get("ok", False):
                p = r["result"]
                return (p[0], p[1])
            if attempt + 1 < 50 and "handle" in where:
                time.sleep(0.1)
        raise RuntimeError(f"ui.at {where}: {r.get('error')}")

    def glide(self, to, dur=0.3):
        """Move the pointer to `to` along an eased path (the app sees every step: hover
        highlights follow it)."""
        frm = self.pos or (1150.0, 620.0)
        n = max(2, int(dur * 40))
        for i in range(1, n + 1):
            t = i / n
            e = t * t * (3 - 2 * t)
            x, y = frm[0] + (to[0] - frm[0]) * e, frm[1] + (to[1] - frm[1]) * e
            self.ui("ui.move", {"x": x, "y": y})
            self.mark(cursor=[x, y])
            time.sleep(dur / n)
        self.pos = to

    def click(self, to, dur=0.3, settle=0.2):
        self.glide(to, dur)
        self.ui("ui.click", {"x": to[0], "y": to[1]})
        self.mark(cursor=list(to), click=True)
        self.wait(settle)
        self.settle_camera()

    def tool(self, command, panel=None):
        """Click a toolbar button (opening its panel's drop-down first when it's in one)."""
        self.settle_camera()
        dialog = self.ui("ui.inspect").get("dialog") or ""
        if "error: Some(" in dialog:
            raise RuntimeError(f"the previous UI operation was refused: {dialog}")
        handles = self.ui("ui.handles")
        if panel and not any(h["name"] == f"toolbar:{command}" for h in handles):
            self.click(self.at(handle=panel), settle=0.3)
        self.click(self.at(handle=f"toolbar:{command}"), settle=0.35)

    def type(self, text, enter=True):
        self.ui("ui.text", {"text": text})
        self.wait(0.35)
        if enter:
            self.ui("ui.key", {"key": "Enter"})
            self.wait(0.5)
            self.settle_camera()

    def key(self, k):
        self.ui("ui.key", {"key": k})
        self.wait(0.15)
        self.settle_camera()

    def view(self, v, settle=0.7):
        if v == "home":
            # The shipping Axis View's Home button, with the same camera/style as v3.
            self.click(self.at(handle="axis_view:home"), settle=0.3)
        else:
            self.ui("ui.view", {"view": v, "animate": True})
        self.wait(settle)
        self.settle_camera()

        if v == "home":
            # Original Home also fitted the model. The current Axis View only
            # rotates: retaining the close sketch camera crops the first extrusion.
            self.ui("ui.view", {"view": "fit"})
            self.settle_camera()
            vp = self.ui("ui.inspect")["viewport"]
            self.ui("ui.scroll", {"x": vp[0] + vp[2] / 2, "y": vp[1] + vp[3] / 2, "delta": -250})
            self.settle_camera()

    def orbit(self, dx, dy=0, steps=40, around=None):
        """A right-button drag in the viewport, as a user orbits. With `around` (a world point)
        the drag starts over it: with the Orbit around cursor preference, the model turns about
        that point and stays in frame."""
        vp = self.ui("ui.inspect").get("viewport") or [250, 126, 1350, 800]
        cx, cy = vp[0] + vp[2] * 0.62, vp[1] + vp[3] * 0.55
        if around is not None:
            cx, cy = self.ui("ui.worldToScreen", {"points": [around]})[0]
        self.glide((cx, cy), 0.25)
        self.mark(drag_start=[cx, cy])
        self.ui("ui.drag", {"x0": cx, "y0": cy, "x1": cx + dx, "y1": cy + dy, "button": "right", "steps": steps})
        self.settle_camera()
        # The pointer moved with the camera (the app plays the drag one event a frame).
        self.mark(drag_end=[cx + dx, cy + dy])
        self.pos = (cx + dx, cy + dy)

    def drag(self, frm, to, button="left", steps=30):
        """Press at `frm`, move to `to` and let go (a window's title bar, or a middle-button pan)."""
        self.glide(frm, 0.25)
        self.mark(drag_start=list(frm))
        self.ui("ui.drag", {"x0": frm[0], "y0": frm[1], "x1": to[0], "y1": to[1], "button": button, "steps": steps})
        self.settle_camera()
        self.wait(0.2)
        self.mark(drag_end=list(to))
        self.pos = tuple(to)

    def settle_camera(self):
        """Wait for queued pointer events, moving dialogs and geometry previews."""
        last, still = None, 0
        for _ in range(1200):
            time.sleep(0.1)
            state = self.ui("ui.inspect")
            current = json.dumps([state.get("camera"), state.get("tool"), self.ui("ui.handles")], sort_keys=True)
            busy = state.get("preview", {}).get("busy", False)
            still = still + 1 if current == last and not busy else 0
            last = current
            if still >= 3:
                return
        raise RuntimeError("the camera/dialog/preview did not settle within 120 seconds")

    def execute(self, command, params):
        self.settle_camera()
        self.mcp.tool("execute", {"command": command, "params": params})
        self.settle_camera()


def datum_sketch(d, base, offset):
    """Create a parametric datum in the real dialog, then select its browser row."""
    d.ui("ui.set", {"showOrigin": True})
    d.tool("construct.plane.offset", panel="panel:SOLID:CONSTRUCT")
    d.click(d.at(plane=base), settle=0.3)
    d.type(offset)
    names = [h["name"] for h in d.ui("ui.handles") if h["name"].startswith("timeline:")]
    plane = names[-1].split(":", 1)[1]
    d.datums = getattr(d, "datums", []) + [plane]
    d.tool("sketch.create")
    d.click(d.at(handle=f"row:{plane}"), settle=0.8)
    d.settle_camera()
    d.ui("ui.set", {"showOrigin": False})


def build_box(d, params):
    d.caption("Parameters over MCP: " + ", ".join(f"{n} {e.split()[0]}" for n, e in params), "MCP tools/call execute parameters.change")
    for name, expr in params:
        d.execute("parameters.change", {"name": name, "expression": expr})
        d.wait(0.25)
    d.wait(0.6)

    d.caption("Create Sketch on the XY plane", "UI click")
    d.tool("sketch.create")
    d.click(d.at(plane="XY"), settle=0.8)
    d.settle_camera()

    d.caption("Center rectangle from the origin", "UI click")
    d.tool("sketch.rectangle.center", panel="family:Rectangle")
    d.click(d.at(sketch=[0, 0]), settle=0.15)
    d.click(d.at(sketch=[40, 30]), dur=0.45, settle=0.2)
    d.key("Escape")

    d.caption("Constraints over MCP: centred, width × depth", "MCP tools/call execute sketch.dimension")
    sk = json.loads(d.mcp.tool("execute", {"command": "sketch.inspect", "params": {}}))
    sk = sk.get("result", sk)
    centre = next((p["id"] for p in sk.get("points", []) if p["id"] != "origin" and abs(p["at"][0]) + abs(p["at"][1]) < 1e-6), None)
    if centre:
        d.execute("sketch.constraint.coincident", {"a": centre, "b": "origin"})
    d.execute("sketch.dimension", {"entities": ["l1"], "value": "width"})
    d.wait(0.3)
    d.execute("sketch.dimension", {"entities": ["l2"], "value": "depth"})
    d.wait(0.8)

    d.caption("Finish Sketch", "UI click")
    d.tool("sketch.finish")
    # The origin planes are in the way of picking faces from here on.
    d.ui("ui.set", {"showOrigin": False})
    d.view("home", 0.6)

    d.caption("Extrude: height", "UI click")
    # The first Fit framed a flat sketch at z=0. Leave room for the upcoming
    # 30 mm height instead of letting its far corner grow above the viewport.
    vp = d.ui("ui.inspect")["viewport"]
    d.ui("ui.scroll", {"x": vp[0] + vp[2] / 2, "y": vp[1] + vp[3] / 2, "delta": -250})
    d.settle_camera()
    d.tool("solid.extrude")
    d.wait(0.3)
    d.type("height")

    d.caption("Fillet the four vertical edges: 6 mm", "UI click")
    d.tool("solid.fillet")
    for p in ([40, -30, 15], [-40, -30, 15]):
        d.click(d.at(world=p), settle=0.15)
    d.orbit(-260, 20)
    for p in ([40, 30, 15], [-40, 30, 15]):
        d.click(d.at(world=p), settle=0.2)
    d.type("6")
    d.view("home", 0.5)

    d.caption("Shell: open the top face, wall thickness", "UI click")
    d.tool("solid.shell", panel="panel:SOLID:MODIFY")
    d.click(d.at(world=[0, 0, 30]), settle=0.3)
    d.type("wall")

    d.caption("Sketch on the side face for a USB port", "UI click")
    d.tool("sketch.create")
    d.click(d.at(world=[40, 10, 20]), settle=0.9)
    d.settle_camera()
    d.tool("sketch.rectangle.center", panel="family:Rectangle")
    d.click(d.at(world=[40, 0, 12]), settle=0.15)
    d.click(d.at(world=[40, 6, 15.5]), dur=0.4, settle=0.2)
    d.key("Escape")
    d.tool("sketch.finish")
    d.view("home", 0.5)

    # Over MCP with the profile named by a point inside it: a profile picked in the Extrude
    # dialog is kept by its index, which a face sketch renumbers when the face changes shape.
    d.caption("Cut the port through the wall", "MCP tools/call execute solid.extrude")
    d.execute("solid.extrude", {"profiles": [{"point": [0, 12]}], "distance": "-wall", "operation": "cut"})
    d.wait(0.6)
    d.orbit(-140, 30)

def run_v2(d):
    """v2: four standoffs from one sketch, then the width changed over MCP."""
    build_box(d, [("width", "80 mm"), ("depth", "60 mm"), ("height", "30 mm"), ("wall", "2 mm")])
    d.caption("Sketch on the inner floor: four standoffs", "UI click")
    d.tool("sketch.create")
    d.click(d.at(world=[6, -9, 2]), settle=0.9)
    d.settle_camera()
    d.tool("sketch.circle.center")
    for x, y in [(-30, -20), (30, -20), (30, 20), (-30, 20)]:
        d.click(d.at(world=[x, y, 2]), dur=0.3, settle=0.1)
        d.click(d.at(world=[x + 3.5, y, 2]), dur=0.2, settle=0.15)
    d.key("Escape")
    d.tool("sketch.finish")

    d.view("home", 0.5)

    d.caption("Extrude the standoffs: 12 mm", "MCP tools/call execute solid.extrude")
    d.execute("solid.extrude", {"profiles": [{"point": [x, y]} for x, y in [(-30, -20), (30, -20), (30, 20), (-30, 20)]], "distance": 12, "operation": "join"})
    d.wait(0.4)

    d.caption("Change one parameter: width 80 → 100 mm", "MCP tools/call execute parameters.change")
    d.execute("parameters.change", {"name": "width", "expression": "100 mm"})
    d.wait(1.8)

    d.caption("", "")
    d.orbit(420, 0, steps=90)
    d.view("home", 1.5)


def select_all_and_type(d, at, text, enter=True):
    """Click a value field, select what is there (Ctrl+A) and type over it."""
    d.click(at, settle=0.15)
    d.ui("ui.key", {"key": "A", "cmd": True})
    d.wait(0.1)
    d.type(text, enter)


def run_v3(d):
    """v3: one standoff placed from the parameters and patterned 2 x 2 in the Rectangular Pattern
    dialog, then width and wall changed in the Parameters dialog."""
    build_box(d, [("width", "80 mm"), ("depth", "60 mm"), ("height", "30 mm"), ("wall", "2 mm"), ("inset", "10 mm")])

    d.caption("Sketch one standoff on the inner floor", "UI click")
    d.tool("sketch.create")
    d.click(d.at(world=[6, -9, 2]), settle=0.9)
    d.settle_camera()
    d.tool("sketch.circle.center")
    d.click(d.at(world=[-30, 20, 2]), dur=0.3, settle=0.1)
    d.click(d.at(world=[-26.5, 20, 2]), dur=0.2, settle=0.15)
    d.key("Escape")

    d.caption("Place it from the parameters: width/2 − inset, depth/2 − inset", "MCP tools/call execute sketch.dimension")
    sk = json.loads(d.mcp.tool("execute", {"command": "sketch.inspect", "params": {}}))
    sk = sk.get("result", sk)
    circles = [c["id"] for c in sk["curves"] if c["type"] == "circle" and not c.get("fixed")]
    if not circles:
        raise RuntimeError(f"no circle in the standoff sketch: {[c['id'] for c in sk['curves']]}")
    circle = circles[0]
    d.execute("sketch.dimension", {"entities": ["origin", f"{circle}.center"], "type": "horizontal", "value": "width / 2 - inset"})
    d.execute("sketch.dimension", {"entities": ["origin", f"{circle}.center"], "type": "vertical", "value": "depth / 2 - inset"})
    d.execute("sketch.dimension", {"entities": [circle], "type": "diameter", "value": "7 mm"})
    d.wait(0.8)
    d.tool("sketch.finish")
    d.view("home", 0.5)

    d.caption("Extrude the standoff: 12 mm", "MCP tools/call execute solid.extrude")
    d.execute("solid.extrude", {"profiles": [[circle]], "distance": 12, "operation": "join"})
    d.wait(0.7)
    standoff = d.ui("ui.handles", {"prefix": "timeline:"})[-1]["name"]

    d.caption("Rectangular Pattern: 2 × 2, spaced width − 2·inset by depth − 2·inset", "UI click")
    d.tool("solid.pattern.rectangular", panel="panel:SOLID:CREATE")
    d.click(d.at(handle="dialog:Object Type"), settle=0.25)
    d.click(d.at(handle="dialog:Object Type:Features"), settle=0.25)
    d.click(d.at(handle=standoff), settle=0.25)
    d.click(d.at(handle="dialog:Direction"), settle=0.15)
    d.click(d.at(world=[0, 30, 30]), settle=0.2)
    d.click(d.at(handle="dialog:Direction 2"), settle=0.15)
    d.click(d.at(world=[-40, 0, 30]), settle=0.2)
    select_all_and_type(d, d.at(handle="dialog:Quantity"), "2", enter=False)
    select_all_and_type(d, d.at(handle="dialog:Spacing"), "width - 2*inset", enter=False)
    select_all_and_type(d, d.at(handle="dialog:Quantity 2"), "2", enter=False)
    select_all_and_type(d, d.at(handle="dialog:Spacing 2"), "-(depth - 2*inset)", enter=False)
    d.wait(0.4)
    d.click(d.at(handle="dialog:OK"), settle=0.4)
    # The original small orbit reveals the corners behind the walls.
    d.orbit(-60, 45)
    # Raise the view enough to see all four 12 mm bosses behind the 30 mm walls.
    d.orbit(0, 35)

    d.key("Escape")

    d.caption("Parameters dialog: width 80 → 100 mm", "UI click")
    d.tool("parameters.change", panel="panel:SOLID:MODIFY")
    d.wait(0.3)
    # Original framing: move the dialog aside and pan the model beside it.
    title = d.at(handle="params:title")
    d.drag(title, (title[0] - 330, title[1] + 20), steps=25)
    vp = d.ui("ui.inspect").get("viewport") or [250, 126, 1350, 800]
    d.drag((vp[0] + vp[2] * 0.62, vp[1] + vp[3] * 0.5), (vp[0] + vp[2] * 0.62 + 280, vp[1] + vp[3] * 0.5), button="middle", steps=25)
    select_all_and_type(d, d.at(handle="param:width"), "100 mm")
    d.wait(1.0)
    d.caption("… and wall 2 → 3 mm", "UI click")
    select_all_and_type(d, d.at(handle="param:wall"), "3 mm")
    d.wait(0.8)
    d.click(d.at(handle="params:close"), settle=0.4)

    d.caption("", "")
    # Keep the original demo's lower three-quarter silhouette.
    d.view("home", 0.5)
    d.wait(1.5)


def sketch_dimension(d, line_at, place_at, value):
    """Sketch Dimension (D) in the real tool: pick a line, place the value, type an expression."""
    d.key("D")
    d.click(d.at(sketch=line_at), settle=0.15)
    d.click(d.at(sketch=place_at), dur=0.3, settle=0.3)
    d.ui("ui.key", {"key": "A", "cmd": True})
    d.type(value)


def swatch(d, row, col):
    """A library swatch of the open Appearance dialog (laid out under its Library row)."""
    lib = d.at(handle="dialog:Library")
    return (lib[0] - 47 + 34 * col, lib[1] + 62 + 34 * row)


def sketch_curves(d):
    """The active sketch's own curves (not projected, not construction)."""
    sk = json.loads(d.mcp.tool("execute", {"command": "sketch.inspect", "params": {}}))
    sk = sk.get("result", sk)
    return [c for c in sk["curves"] if not c.get("fixed") and not c.get("construction")]


def rectangle(d, centre, half):
    """A centre rectangle drawn on the active sketch's face with world-space picks (`half` is
    the corner's offset from `centre`); returns its four sides' ids."""
    before = {c["id"] for c in sketch_curves(d)}
    d.tool("sketch.rectangle.center", panel="family:Rectangle")
    d.click(d.at(world=centre), settle=0.15)
    d.click(d.at(world=[c + h for c, h in zip(centre, half)]), dur=0.35, settle=0.2)
    d.key("Escape")
    sides = [c["id"] for c in sketch_curves(d) if c["id"] not in before and c["type"] == "line"]
    if len(sides) != 4:
        raise RuntimeError(f"the rectangle at {centre} did not draw four sides: {sides}")
    return sides


def extrude(d, loops, distance, operation="cut"):
    """Extrude the last sketch's profiles bounded by these curve loops. Over MCP, by curves:
    the profiles are found again on every rebuild, whatever a parameter does to the face."""
    d.execute("solid.extrude", {"profiles": [{"curves": loop} for loop in loops], "distance": distance, "operation": operation})
    d.wait(0.5)


def pattern(d, feature, rows):
    """Rectangular Pattern of one feature in the real dialog. `rows` is [(edge world point,
    quantity, spacing), …] for Direction and Direction 2."""
    d.tool("solid.pattern.rectangular", panel="panel:SOLID:CREATE")
    d.click(d.at(handle="dialog:Object Type"), settle=0.25)
    d.click(d.at(handle="dialog:Object Type:Features"), settle=0.25)
    d.click(d.at(handle=feature), settle=0.25)
    for n, (edge, quantity, spacing) in enumerate(rows):
        suffix = "" if n == 0 else " 2"
        d.click(d.at(handle=f"dialog:Direction{suffix}"), settle=0.15)
        d.click(d.at(world=edge), settle=0.2)
        select_all_and_type(d, d.at(handle=f"dialog:Quantity{suffix}"), str(quantity), enter=False)
        select_all_and_type(d, d.at(handle=f"dialog:Spacing{suffix}"), spacing, enter=False)
    d.wait(0.5)
    d.click(d.at(handle="dialog:OK"), settle=0.4)


def last_feature(d):
    return d.ui("ui.handles", {"prefix": "timeline:"})[-1]["name"]


# Raspberry Pi 4 / 5 board: 85 x 56 mm, M2.5 holes on a 58 x 49 mm rectangle, 3.5 mm in from
# one short edge and both long edges. The case is centred on the board with 3 mm clearance.
HOLE = (-(85 / 2 - 3.5), -(56 / 2 - 3.5))


def colour(d):
    """0.6: drag ABS red from the Appearance dialog onto the case."""
    d.caption("Give it a colour")
    D = 33.0
    d.key("A")
    d.wait(0.3)
    d.drag(swatch(d, 1, 5), d.at(world=[0, -D, 18]), steps=30)
    # The dialog adds a note under the swatches: let it lay out before finding OK.
    d.glide((1500, 700), 0.3)
    d.settle_camera()
    d.click(d.at(handle="dialog:OK"), settle=0.4)
    if "Appearance" in (d.ui("ui.inspect").get("dialog") or ""):
        raise RuntimeError("the Appearance dialog did not close")


def engrave(d):
    """0.8: "Pi 4" in DejaVu Sans Bold, engraved 0.6 mm into the floor behind the vents."""
    d.caption("Engrave “Pi 4” in the floor")
    d.tool("sketch.create")
    d.click(d.at(world=[-5, 18, 2]), settle=0.9)
    d.settle_camera()
    d.tool("sketch.text", panel="panel:SKETCH:CREATE")
    d.click(d.at(world=[-9, 20, 2]), settle=0.4)
    d.ui("ui.text", {"text": "Pi 4"})
    d.wait(0.6)
    d.ui("ui.key", {"key": "Enter"})
    d.wait(0.4)
    d.settle_camera()
    d.caption("Font: DejaVu Sans Bold, 6 mm")
    link = next(l["id"] for l in inspect_sketch(d)["links"] if l["kind"] == "text")
    d.execute("sketch.edit_text", {"link": link, "font": "DejaVu Sans Bold", "height": 6})
    # Centre it on the free floor between the vents and the back wall.
    sk = inspect_sketch(d)
    pts = [c[k] for c in sk["curves"] if c.get("link") == link for k in ("start_at", "end_at") if k in c]
    x0, x1 = min(p[0] for p in pts), max(p[0] for p in pts)
    y0, y1 = min(p[1] for p in pts), max(p[1] for p in pts)
    at = next(l["source"]["at"] for l in sk["links"] if l["id"] == link)
    d.execute("sketch.edit_text", {"link": link, "at": [round(at[0] - (x0 + x1) / 2, 2), round(at[1] + 22.5 - (y0 + y1) / 2, 2)]})
    d.wait(0.6)
    sk = inspect_sketch(d)
    letters = {c["id"] for c in sk["curves"] if c.get("link") == link}
    profiles = [p for p in sk["profiles"] if set(p["curves"]) <= letters]
    counters = {frozenset(h) for p in profiles for h in p["holes"]}
    loops = [p["curves"] for p in profiles if frozenset(p["curves"]) not in counters]
    d.tool("sketch.finish")
    d.view("home", 0.4)
    extrude(d, loops, "-0.6 mm")


def material(d):
    """0.8: a PETG physical material defined in the design and assigned to the case over MCP,
    then the mass it gives in Properties (the body's browser menu)."""
    d.caption("PETG: a material of its own, 1.27 g/cm³")
    d.execute("material.create", {"name": "PETG", "density_g_cm3": 1.27, "color": "#2a8696"})
    d.execute("material.assign", {"bodies": ["Body1"], "material": "PETG"})
    d.wait(0.6)
    d.caption("Properties: material and mass")
    body = d.at(handle="row:Body1")
    d.glide(body, 0.3)
    d.ui("ui.click", {"x": body[0], "y": body[1], "button": "right"})
    d.mark(cursor=list(body), click=True)
    d.wait(0.4)
    d.settle_camera()
    d.click(d.at(handle="menu:ui.properties"), settle=0.5)
    # Real seconds, not scaled by --speed: long enough to read at playback speed.
    time.sleep(PROPERTIES_HOLD)
    d.click(PROPERTIES_CLOSE, settle=0.3)
    d.key("Escape")


# Seconds the Properties window stays up (about 1.5 s at 8-9x playback).
PROPERTIES_HOLD = 13.0
# The Properties window's close button (it opens centred on the 1920 x 1080 window).
PROPERTIES_CLOSE = (1112.0, 387.0)


def inspect_sketch(d):
    sk = json.loads(d.mcp.tool("execute", {"command": "sketch.inspect", "params": {}}))
    return sk.get("result", sk)


def run_pi(d, release="0.8"):
    """pi (the 0.8 README demo): a case for a Raspberry Pi 4. A sketch dimensioned from the
    board's parameters, extrude, fillets and shell, one standoff with its M2.5 hole patterned on
    the board's 58 x 49 mm hole rectangle, port cut-outs on a side and an end, a row of vent
    slots, then the height changed in the Parameters dialog. 0.8 also rounds the top rim,
    engraves "Pi 4" in a font into the floor and makes the case PETG (a material defined
    in the design) with its mass in Properties; 0.6 (`--story pi-0.6`) gave it a colour instead."""
    v08 = release == "0.8"
    d.caption("A case for a Raspberry Pi 4")
    for name, expr in [("board_w", "85 mm"), ("board_d", "56 mm"), ("wall", "2 mm"), ("height", "24 mm")]:
        d.execute("parameters.change", {"name": name, "expression": expr})
    d.wait(0.3)
    W, D, H = 47.5, 33.0, 24.0

    d.caption("Sketch and dimension")
    d.tool("sketch.create")
    d.click(d.at(plane="XY"), settle=0.8)
    d.settle_camera()
    d.tool("sketch.rectangle.center", panel="family:Rectangle")
    d.click(d.at(sketch=[0, 0]), settle=0.15)
    d.click(d.at(sketch=[W, D]), dur=0.45, settle=0.2)
    d.key("Escape")
    sketch_dimension(d, [0, D], [0, D + 8], "board_w + 10 mm")
    sketch_dimension(d, [-W, 0], [-W - 8, 0], "board_d + 10 mm")
    d.key("Escape")
    d.wait(0.6)

    d.caption("Extrude to height")
    d.tool("sketch.finish")
    d.ui("ui.set", {"showOrigin": False})
    d.view("home", 0.5)
    # Home frames the flat sketch: step back to leave room for the walls.
    vp = d.ui("ui.inspect")["viewport"]
    d.ui("ui.scroll", {"x": vp[0] + vp[2] / 2, "y": vp[1] + vp[3] / 2, "delta": -250})
    d.settle_camera()
    d.tool("solid.extrude")
    d.wait(0.2)
    d.type("height")

    d.caption("Round the corners")
    d.tool("solid.fillet")
    for p in ([W, -D, H / 2], [-W, -D, H / 2]):
        d.click(d.at(world=p), settle=0.15)
    d.orbit(-260, 20, around=[0, 0, H / 2])
    for p in ([W, D, H / 2], [-W, D, H / 2]):
        d.click(d.at(world=p), settle=0.2)
    d.type("5")
    d.view("home", 0.4)

    d.caption("Shell it, open on top")
    d.tool("solid.shell", panel="panel:SOLID:MODIFY")
    d.click(d.at(world=[0, 0, H]), settle=0.3)
    d.type("wall")

    if v08:
        # Before the port cuts, so the rim's edges are whole when it is picked.
        d.caption("Round the rim in one pick")
        d.tool("solid.fillet")
        d.click(d.at(world=[0, D, H]), settle=0.3)
        d.type("1")

    d.caption("A standoff with an M2.5 hole")
    hx, hy = HOLE
    d.tool("sketch.create")
    d.click(d.at(world=[-5, 18, 2]), settle=0.9)
    d.settle_camera()
    # Zoom in on the hole (zoom goes to the pointer) so the 2.5 mm circle is drawn at a size.
    at = d.at(world=[hx, hy, 2])
    d.glide(at, 0.3)
    for _ in range(3):
        d.ui("ui.scroll", {"x": at[0], "y": at[1], "delta": 250})
        d.settle_camera()
    d.tool("sketch.circle.center")
    d.click(d.at(world=[hx, hy, 2]), dur=0.3, settle=0.1)
    d.click(d.at(world=[hx + 3, hy, 2]), dur=0.2, settle=0.15)
    d.key("Escape")
    d.tool("sketch.circle.center")
    d.click(d.at(world=[hx, hy, 2]), dur=0.2, settle=0.1)
    d.click(d.at(world=[hx + 1.25, hy, 2]), dur=0.2, settle=0.15)
    d.key("Escape")
    circles = sorted((c for c in sketch_curves(d) if c["type"] == "circle"), key=lambda c: -c.get("radius", 0))
    if len(circles) != 2:
        raise RuntimeError(f"expected two circles in the standoff sketch: {circles}")
    outer, inner = circles[0]["id"], circles[1]["id"]
    try:
        d.execute("sketch.constraint.coincident", {"a": f"{inner}.center", "b": f"{outer}.center"})
    except RuntimeError:
        pass  # the second click already snapped to the first centre
    d.execute("sketch.dimension", {"entities": ["origin", f"{outer}.center"], "type": "horizontal", "value": "board_w / 2 - 3.5 mm"})
    d.execute("sketch.dimension", {"entities": ["origin", f"{outer}.center"], "type": "vertical", "value": "board_d / 2 - 3.5 mm"})
    d.execute("sketch.dimension", {"entities": [outer], "type": "diameter", "value": "6 mm"})
    d.execute("sketch.dimension", {"entities": [inner], "type": "diameter", "value": "2.5 mm"})
    d.wait(0.5)
    d.tool("sketch.finish")
    d.view("home", 0.4)
    extrude(d, [[outer]], "4 mm", "join")
    standoff = last_feature(d)

    d.caption("Pattern it on 58 × 49 mm")
    # Directions from the bottom edges, which a height change leaves where they are.
    pattern(d, standoff, [([0, -D, 0], 2, "58 mm"), ([W, 0, 0], 2, "49 mm")])
    d.orbit(-60, 35, around=[0, 0, 4])

    d.caption("Cut the side ports")
    d.view("home", 0.4)
    d.tool("sketch.create")
    d.click(d.at(world=[10, -D, 16]), settle=0.9)
    d.settle_camera()
    ports = [(-31.3, 5, 2), (-16.5, 4, 2), (-3, 4, 2)]  # USB-C, 2x micro-HDMI: centre x, half w, half h
    loops = [rectangle(d, [x, -D, 9.5], [hw, 0, hh]) for x, hw, hh in ports]
    d.tool("sketch.finish")
    d.view("home", 0.4)
    extrude(d, loops, "-wall")

    d.caption("Open the end for USB and Ethernet")
    d.tool("sketch.create")
    d.click(d.at(world=[W, 0, 4]), settle=0.9)
    d.settle_camera()
    loop = rectangle(d, [W, 0, 15], [0, 25, 7])
    d.tool("sketch.finish")
    d.view("home", 0.4)
    extrude(d, [loop], "-wall")

    d.caption("Vent slots in the floor")
    d.tool("sketch.create")
    d.click(d.at(world=[-5, 18, 2]), settle=0.9)
    d.settle_camera()
    loop = rectangle(d, [-8, -14, 2], [-14, 2.5, 0])
    d.tool("sketch.finish")
    d.view("home", 0.4)
    extrude(d, [loop], "-wall")
    pattern(d, last_feature(d), [([W, 0, 0], 5, "7 mm")])

    if v08:
        engrave(d)
        material(d)
    else:
        colour(d)

    d.caption("Change a parameter: height 24 → 32 mm")
    d.tool("parameters.change", panel="panel:SOLID:CONFIGURE")
    d.wait(0.3)
    vp = d.ui("ui.inspect").get("viewport") or [250, 126, 1350, 800]
    d.drag((vp[0] + vp[2] * 0.55, vp[1] + vp[3] * 0.7), (vp[0] + vp[2] * 0.55 + 260, vp[1] + vp[3] * 0.7 - 60), button="middle", steps=25)
    select_all_and_type(d, d.at(handle="param:height"), "32 mm")
    d.wait(1.5)
    d.click(d.at(handle="params:close"), settle=0.4)

    d.caption("")
    d.view("home", 0.5)
    vp = d.ui("ui.inspect")["viewport"]
    d.ui("ui.scroll", {"x": vp[0] + vp[2] / 2, "y": vp[1] + vp[3] / 2, "delta": 120})
    d.settle_camera()
    # Swing round to the far side and back to the three-quarter view of the ports.
    # Off the model after each drag, so no face is left highlighted under the pointer.
    away = (vp[0] + vp[2] - 120, vp[1] + vp[3] - 90)
    d.orbit(260, -10, steps=110, around=[0, 0, 10])
    d.glide(away, 0.3)
    d.settle_camera()
    d.orbit(-260, 10, steps=110, around=[0, 0, 10])
    d.glide(away, 0.3)
    d.settle_camera()
    # Frame the finished 32 mm case whole, in the same three-quarter direction.
    d.ui("ui.view", {"view": "fit", "animate": True})
    d.settle_camera()
    d.wait(1.0)


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--port", type=int, default=7878, help="the app's --control port")
    ap.add_argument("--cli", default="solidcraft-cli", help="path to solidcraft-cli")
    ap.add_argument("--log", help="write a JSON line per caption and pointer event")
    ap.add_argument("--story", choices=["v2", "v3", "pi", "pi-0.6"], default="pi", help="v2: standoffs from one sketch, width over MCP; v3: one standoff patterned, Parameters dialog; pi: the 0.8 README demo, a Raspberry Pi 4 case; pi-0.6: the 0.6 one")
    ap.add_argument("--until", type=int, default=10**6, help="stop before step N (trying the script out)")
    ap.add_argument("--speed", type=float, default=0.55, help="scale the pauses (2 = twice as long)")
    ap.add_argument("--save", help="save the completed healthy enclosure and measured proof")
    ap.add_argument("--minimum-caption", type=float, default=MIN_CAPTION, help="minimum real-time caption duration before playback acceleration")
    a = ap.parse_args()
    if not math.isfinite(a.speed) or a.speed <= 0 or not math.isfinite(a.minimum_caption) or a.minimum_caption < 0:
        ap.error("pause scale must be positive and caption duration nonnegative")
    argv = [a.cli, "mcp", "--connect", f"127.0.0.1:{a.port}"]
    if a.save:
        # MCP file access is limited to the open design's folder and the recovery folder.
        argv += ["--allow-path", os.path.dirname(os.path.abspath(a.save))]
    mcp = Mcp(argv)
    mcp.send("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "enclosure-demo-v2", "version": "2"}})
    mcp.send("notifications/initialized", notify=True)
    d = Demo(a.port, mcp, open(a.log, "w") if a.log else None, a.speed, a.minimum_caption)
    # The pi story's camera moves orbit about the point under the pointer (Preferences > Navigation).
    d.ui("ui.prefs", {"language": "en", "theme": "dark", "ui_scale": 1, "nav": "fusion", "orbit_around": "cursor" if a.story.startswith("pi") else "view_centre", "button": "apply"})
    d.ui("ui.resize", {"width": 1920, "height": 1080})
    d.ui("ui.home", {"open": False})
    d.ui("ui.documents", {"action": "new"})
    d.ui("ui.home", {"open": False})
    d.ui("ui.set", {"showOrigin": True})
    d.settle_camera()
    d.stop = a.until - 1
    t0 = time.time()
    try:
        {"v2": run_v2, "v3": run_v3, "pi": run_pi, "pi-0.6": lambda d: run_pi(d, "0.6")}[a.story](d)
    except StopIteration:
        pass
    if a.story.startswith("pi") and a.until == 10**6:
        # Check the finished case without changing what is on screen.
        d.settle_camera()
        design = d.ui("document.inspect", {"measure": True})
        bodies = design.get("bodies", [])
        if len(bodies) != 1 or any(f.get("error") or f.get("warning") for f in design.get("timeline", [])):
            raise RuntimeError(f"the finished case is unhealthy: {design}")
        box = bodies[0].get("bbox", {})
        want = {"min": [-47.5, -33, 0], "max": [47.5, 33, 32]}
        if any(abs(v - w) > 1e-5 for side in want for v, w in zip(box.get(side, []), want[side])):
            raise RuntimeError(f"the case did not rebuild to 95 x 66 x 32 mm: {box}")
        if a.save:
            # The finished case, as built through the UI (tests/action_matrix uses it).
            d.execute("file.save_as", {"path": a.save, "format": "json"})
            with open(a.save + ".measure.json", "w") as proof:
                json.dump(design, proof, indent=2)
    elif a.save and a.until == 10**6:
        # Keep this health/save/reopen proof in the raw take, outside public playback.
        d.mark(private_start=True)
        d.settle_camera()
        design = d.ui("document.inspect", {"measure": True})
        if len(design.get("bodies", [])) != 1 or any(f.get("error") or f.get("warning") for f in design.get("timeline", [])):
            raise RuntimeError(f"the completed enclosure is unhealthy: {design}")
        expected_width = 100
        bbox = design["bodies"][0]["bbox"]
        if any(abs(v - want) > 1e-5 for side, expected in [("min", [-expected_width / 2, -30, 0]), ("max", [expected_width / 2, 30, 30])] for v, want in zip(bbox[side], expected)):
            raise RuntimeError(f"the enclosure did not rebuild to 100 x 60 x 30 mm: {design}")
        values = {p["name"]: p.get("value") for p in design.get("params", []) if not p.get("error")}
        for name, expected in [("width", 100), ("wall", 3 if a.story == "v3" else 2)]:
            if values.get(name) != expected:
                raise RuntimeError(f"the final {name} parameter is not {expected}: {design}")
        d.execute("file.save_as", {"path": a.save, "format": "json"})
        saved = json.load(open(a.save))
        fillets = [f for f in saved["features"] if f.get("type") == "fillet"]
        if len(fillets) != 1 or len(fillets[0].get("edges", [])) != 4:
            raise RuntimeError("the enclosure did not retain all four selected corner fillets")
        with open(a.save + ".measure.json", "w") as proof:
            json.dump(design, proof, indent=2)
        # Reopen the saved design for the ending: the live pattern preview can leave
        # stale display edges behind. This is the actual app, not a rendered substitute.
        d.ui("ui.documents", {"action": "close", "force": True})
        d.ui("ui.file", {"path": a.save})
        for _ in range(1200):
            if not d.ui("ui.inspect").get("file_busy", False):
                break
            time.sleep(0.1)
        else:
            raise RuntimeError("the saved enclosure did not reopen")
        d.ui("ui.home", {"open": False})
        d.settle_camera()
        d.ui("ui.set", {"showOrigin": False, "showSketches": False})
        d.mark(private_end=True)
        d.ui("ui.view", {"view": "home"})
        d.settle_camera()
        d.orbit(0, 80)
        d.ui("ui.view", {"view": "fit"})
        d.settle_camera()
        vp = d.ui("ui.inspect")["viewport"]
        d.ui("ui.scroll", {"x": vp[0] + vp[2] / 2, "y": vp[1] + vp[3] / 2, "delta": -120})
        d.settle_camera()
        d.glide((1500, 730), 0.4)
    d.mark(caption="", via="")
    mcp.close()
    print(f"done in {time.time() - t0:.1f}s", file=sys.stderr)


if __name__ == "__main__":
    main()
