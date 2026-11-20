# Writes knob.FCStd: a PartDesign body called "Knob" made by revolving a sketch, the body placed
# 50 mm along X and turned 30 degrees about Z. The profile on the body's XZ plane is a 12 x 8 mm
# rectangle beside the Z axis whose outer top corner is rounded by a 3 mm arc; it turns 270
# degrees about Z. A Groove then turns a 2 x 2 mm notch out of the top through 90 degrees, and a
# 1 mm chamfer breaks the bottom outer edge.
# Run inside FreeCAD as an external program, from this directory: `freecadcmd make_knob_fcstd.py`
# (made with FreeCAD from conda-forge through `pixi exec -s freecad freecadcmd make_knob_fcstd.py`).
import math
import os

import FreeCAD
import Part
import Sketcher

V = FreeCAD.Vector

doc = FreeCAD.newDocument("Knob")
body = doc.addObject("PartDesign::Body", "Knob")
body.Placement = FreeCAD.Placement(V(50, 0, 0), FreeCAD.Rotation(V(0, 0, 1), 30))

# On XZ the sketch's u is the model X axis and its v the model Z axis.
sk = body.newObject("Sketcher::SketchObject", "Profile")
sk.AttachmentSupport = (doc.getObject("XZ_Plane"), [""])
sk.MapMode = "FlatFace"
w, h, r = 12.0, 8.0, 3.0
sk.addGeometry(Part.LineSegment(V(0, 0, 0), V(w, 0, 0)), False)  # 0 bottom
sk.addGeometry(Part.LineSegment(V(w, 0, 0), V(w, h - r, 0)), False)  # 1 outer side
sk.addGeometry(Part.ArcOfCircle(Part.Circle(V(w - r, h - r, 0), V(0, 0, 1), r), 0, math.pi / 2), False)  # 2
sk.addGeometry(Part.LineSegment(V(w - r, h, 0), V(0, h, 0)), False)  # 3 top
sk.addGeometry(Part.LineSegment(V(0, h, 0), V(0, 0, 0)), False)  # 4 on the axis
sk.addConstraint(Sketcher.Constraint("Coincident", 0, 2, 1, 1))
sk.addConstraint(Sketcher.Constraint("Coincident", 3, 2, 4, 1))
sk.addConstraint(Sketcher.Constraint("Coincident", 4, 2, 0, 1))
sk.addConstraint(Sketcher.Constraint("Coincident", 0, 1, -1, 1))
sk.addConstraint(Sketcher.Constraint("Horizontal", 0))
sk.addConstraint(Sketcher.Constraint("Horizontal", 3))
sk.addConstraint(Sketcher.Constraint("Vertical", 1))
sk.addConstraint(Sketcher.Constraint("PointOnObject", 4, 1, -2))
sk.addConstraint(Sketcher.Constraint("Tangent", 1, 2, 2, 1))
sk.addConstraint(Sketcher.Constraint("Tangent", 2, 2, 3, 1))
sk.addConstraint(Sketcher.Constraint("DistanceX", 0, 1, 0, 2, w))
sk.addConstraint(Sketcher.Constraint("DistanceY", 4, 2, 4, 1, h))
sk.addConstraint(Sketcher.Constraint("Radius", 2, r))

rev = body.newObject("PartDesign::Revolution", "Revolution")
rev.Profile = sk
rev.ReferenceAxis = (sk, ["V_Axis"])
rev.Angle = 270
sk.Visibility = False
doc.recompute()

notch = body.newObject("Sketcher::SketchObject", "NotchSketch")
notch.AttachmentSupport = (doc.getObject("XZ_Plane"), [""])
notch.MapMode = "FlatFace"
c = [V(2, 6, 0), V(4, 6, 0), V(4, 8, 0), V(2, 8, 0)]
for i in range(4):
    notch.addGeometry(Part.LineSegment(c[i], c[(i + 1) % 4]), False)
for i in range(4):
    notch.addConstraint(Sketcher.Constraint("Coincident", i, 2, (i + 1) % 4, 1))
groove = body.newObject("PartDesign::Groove", "Groove")
groove.Profile = notch
groove.ReferenceAxis = (notch, ["V_Axis"])
groove.Angle = 90
notch.Visibility = False
doc.recompute()
assert groove.isValid(), "groove failed"

# Chamfer the bottom outer arc: the circular edge of radius w at z = 0.
bottom = []
for i, e in enumerate(groove.Shape.Edges):
    c = e.Curve
    if isinstance(c, Part.Circle) and abs(c.Radius - w) < 1e-9 and abs(c.Center.z) < 1e-9 and e.Length > 50:
        bottom.append("Edge%d" % (i + 1))
assert len(bottom) == 1, [(type(e.Curve).__name__, e.BoundBox) for e in groove.Shape.Edges]
ch = body.newObject("PartDesign::Chamfer", "Chamfer")
ch.Base = (groove, bottom)
ch.Size = 1
doc.recompute()

assert ch.isValid(), "chamfer failed"
print("volume", ch.Shape.Volume, "revolution", rev.Shape.Volume, "groove", groove.Shape.Volume, ch.Shape.BoundBox)
doc.saveAs(os.path.join(os.getcwd(), "knob.FCStd"))
