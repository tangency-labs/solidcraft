# Writes shelled.FCStd: a PartDesign body called "Housing" whose history ends in a Thickness
# (shell), which SolidCraft does not rebuild, so the file's solids come in with its sketches
# beside them. A fully constrained 40 x 20 mm rectangle on the XY plane is padded 10 mm; a
# datum plane 10 mm up holds a 6 mm circle centred at (20, 10) pocketed 4 mm; a 5 x 5 mm
# rectangle on the XZ plane is used by nothing; the shell takes the top face off, 1 mm thick.
# Run inside FreeCAD as an external program, from this directory:
# `freecadcmd make_shelled_fcstd.py` (made with FreeCAD from conda-forge through
# `pixi exec -s freecad freecadcmd make_shelled_fcstd.py`).
import os

import FreeCAD
import Part
import Sketcher
from FreeCAD import Placement, Rotation

V = FreeCAD.Vector


def rect(sk, w, h, x0=0, y0=0):
    pts = [V(x0, y0, 0), V(x0 + w, y0, 0), V(x0 + w, y0 + h, 0), V(x0, y0 + h, 0)]
    g = [sk.addGeometry(Part.LineSegment(pts[i], pts[(i + 1) % 4]), False) for i in range(4)]
    for i in range(4):
        sk.addConstraint(Sketcher.Constraint("Coincident", g[i], 2, g[(i + 1) % 4], 1))
    sk.addConstraint(Sketcher.Constraint("Horizontal", g[0]))
    sk.addConstraint(Sketcher.Constraint("Horizontal", g[2]))
    sk.addConstraint(Sketcher.Constraint("Vertical", g[1]))
    sk.addConstraint(Sketcher.Constraint("Vertical", g[3]))
    sk.addConstraint(Sketcher.Constraint("DistanceX", g[0], 1, g[0], 2, w))
    sk.addConstraint(Sketcher.Constraint("DistanceY", g[1], 1, g[1], 2, h))
    sk.addConstraint(Sketcher.Constraint("DistanceX", -1, 1, g[0], 1, x0))
    sk.addConstraint(Sketcher.Constraint("DistanceY", -1, 1, g[0], 1, y0))


def origin_plane(body, role):
    return next(f for f in body.Origin.OriginFeatures if f.Role == role)


doc = FreeCAD.newDocument("Shelled")
body = doc.addObject("PartDesign::Body", "Body")
body.Label = "Housing"

base = body.newObject("Sketcher::SketchObject", "BaseSketch")
base.AttachmentSupport = (origin_plane(body, "XY_Plane"), [""])
base.MapMode = "FlatFace"
rect(base, 40, 20)
pad = body.newObject("PartDesign::Pad", "Pad")
pad.Profile = base
pad.Length = 10
doc.recompute()

top = body.newObject("PartDesign::Plane", "TopPlane")
top.AttachmentSupport = [(origin_plane(body, "XY_Plane"), "")]
top.MapMode = "FlatFace"
top.AttachmentOffset = Placement(V(0, 0, 10), Rotation())
doc.recompute()
hole = body.newObject("Sketcher::SketchObject", "HoleSketch")
hole.AttachmentSupport = (top, [""])
hole.MapMode = "FlatFace"
c = hole.addGeometry(Part.Circle(V(20, 10, 0), V(0, 0, 1), 3), False)
hole.addConstraint(Sketcher.Constraint("Diameter", c, 6))
hole.addConstraint(Sketcher.Constraint("DistanceX", -1, 1, c, 3, 20))
hole.addConstraint(Sketcher.Constraint("DistanceY", -1, 1, c, 3, 10))
doc.recompute()
pocket = body.newObject("PartDesign::Pocket", "Pocket")
pocket.Profile = hole
pocket.Length = 4
doc.recompute()

reference = body.newObject("Sketcher::SketchObject", "Reference")
reference.AttachmentSupport = (origin_plane(body, "XZ_Plane"), [""])
reference.MapMode = "FlatFace"
rect(reference, 5, 5, 50, 0)
doc.recompute()

shell = body.newObject("PartDesign::Thickness", "Shell")
shell.Base = (pocket, ["Face6"])
shell.Value = 1
doc.recompute()
doc.saveAs(os.path.join(os.getcwd(), "shelled.FCStd"))
