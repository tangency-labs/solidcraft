# Writes bracket.FCStd: a PartDesign body built from sketches, called "Bracket". A 40 x 20 mm
# rectangle on the XY plane padded 10 mm, a 6 mm diameter hole pocketed through it, and a 2 mm
# fillet on the four vertical edges. Volume 40*20*10 - pi*3^2*10 - 4*(4 - pi)*10 = 8000 - 90 pi
# - 160 + 40 pi = 7840 - 50 pi. Run inside FreeCAD as an external program, from this directory:
# `freecadcmd make_bracket_fcstd.py` (made with FreeCAD from conda-forge through
# `pixi exec -s freecad freecadcmd make_bracket_fcstd.py`).
import os

import FreeCAD
import Part
import Sketcher

V = FreeCAD.Vector

doc = FreeCAD.newDocument("Bracket")
body = doc.addObject("PartDesign::Body", "Bracket")

# Base sketch: a fully constrained 40 x 20 rectangle with a corner at the origin.
base = body.newObject("Sketcher::SketchObject", "Profile")
base.AttachmentSupport = (doc.getObject("XY_Plane"), [""])
base.MapMode = "FlatFace"
corners = [V(0, 0, 0), V(40, 0, 0), V(40, 20, 0), V(0, 20, 0)]
for i in range(4):
    base.addGeometry(Part.LineSegment(corners[i], corners[(i + 1) % 4]), False)
for i in range(4):
    base.addConstraint(Sketcher.Constraint("Coincident", i, 2, (i + 1) % 4, 1))
base.addConstraint(Sketcher.Constraint("Horizontal", 0))
base.addConstraint(Sketcher.Constraint("Horizontal", 2))
base.addConstraint(Sketcher.Constraint("Vertical", 1))
base.addConstraint(Sketcher.Constraint("Vertical", 3))
base.addConstraint(Sketcher.Constraint("Coincident", 0, 1, -1, 1))
base.addConstraint(Sketcher.Constraint("DistanceX", 0, 1, 0, 2, 40))
base.addConstraint(Sketcher.Constraint("DistanceY", 1, 1, 1, 2, 20))

pad = body.newObject("PartDesign::Pad", "Pad")
pad.Profile = base
pad.Length = 10
base.Visibility = False

# A 6 mm hole at (20, 10), cut through all from a sketch on the XY plane.
hole = body.newObject("Sketcher::SketchObject", "HoleSketch")
hole.AttachmentSupport = (doc.getObject("XY_Plane"), [""])
hole.MapMode = "FlatFace"
hole.addGeometry(Part.Circle(V(20, 10, 0), V(0, 0, 1), 3), False)
hole.addConstraint(Sketcher.Constraint("Diameter", 0, 6))
hole.addConstraint(Sketcher.Constraint("DistanceX", -1, 1, 0, 3, 20))
hole.addConstraint(Sketcher.Constraint("DistanceY", -1, 1, 0, 3, 10))

pocket = body.newObject("PartDesign::Pocket", "Pocket")
pocket.Profile = hole
pocket.Type = "ThroughAll"
pocket.Reversed = True
hole.Visibility = False
doc.recompute()

# Fillet the four vertical edges of the pocketed block.
vertical = []
for i, e in enumerate(pocket.Shape.Edges):
    a, b = e.Vertexes[0].Point, e.Vertexes[-1].Point
    if a.x == b.x and a.y == b.y and a.x in (0, 40) and a.y in (0, 20):
        vertical.append("Edge%d" % (i + 1))
assert len(vertical) == 4, vertical
fillet = body.newObject("PartDesign::Fillet", "Fillet")
fillet.Base = (pocket, vertical)
fillet.Radius = 2
doc.recompute()

assert fillet.isValid(), "fillet failed"
print("volume", fillet.Shape.Volume)
doc.saveAs(os.path.join(os.getcwd(), "bracket.FCStd"))
