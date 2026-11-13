# Writes plate.FCStd: a PartDesign body called "Plate", saved from FreeCAD's window so the body's
# tip stays shown as FreeCAD shows it. A 60 x 30 mm sketch with two 4 mm holes padded 5 mm; a
# slot pocketed through all along the X axis from a sketch on the YZ plane, then mirrored; a 2 mm
# hole pocketed 2 mm and repeated four times about Z; a 2 mm hole pocketed 1 mm and repeated three
# times along X; a shape binder of the first sketch; and the four vertical corners filleted 2 mm.
# Run from this directory with FreeCAD's window offscreen:
# `QT_QPA_PLATFORM=offscreen freecad make_plate_fcstd.py` (made with FreeCAD from conda-forge
# through `pixi exec -s freecad freecad make_plate_fcstd.py`).
import os

import FreeCAD
import FreeCADGui
import Part
import Sketcher

V = FreeCAD.Vector
LOG = open(os.environ.get("PLATE_LOG", os.devnull), "w")


def log(*a):
    print(*a, file=LOG, flush=True)


doc = FreeCAD.newDocument("Plate")
body = doc.addObject("PartDesign::Body", "Plate")
origin = {f.Role: f for f in body.Origin.OriginFeatures}


def sketch(name, plane):
    sk = body.newObject("Sketcher::SketchObject", name)
    sk.AttachmentSupport = (origin[plane], [""])
    sk.MapMode = "FlatFace"
    return sk


def rectangle(sk, x0, y0, x1, y1):
    c = [V(x0, y0, 0), V(x1, y0, 0), V(x1, y1, 0), V(x0, y1, 0)]
    first = sk.GeometryCount
    for i in range(4):
        sk.addGeometry(Part.LineSegment(c[i], c[(i + 1) % 4]), False)
    for i in range(4):
        sk.addConstraint(Sketcher.Constraint("Coincident", first + i, 2, first + (i + 1) % 4, 1))


def solid(kind, name):
    # Each solid feature builds on the one before: it becomes the body's tip.
    o = body.newObject(kind, name)
    body.Tip = o
    return o


def volume(o):
    doc.recompute()
    log(o.Name, o.isValid(), o.Shape.Volume if not o.Shape.isNull() else None)
    assert o.isValid(), o.Name
    return o.Shape.Volume


profile = sketch("Profile", "XY_Plane")
rectangle(profile, -30, -15, 30, 15)
for x in (-20, 20):
    profile.addGeometry(Part.Circle(V(x, 0, 0), V(0, 0, 1), 2), False)
pad = solid("PartDesign::Pad", "Pad")
pad.Profile = profile
pad.Length = 5
profile.Visibility = False
log("pad", volume(pad))

slot = sketch("SlotSketch", "YZ_Plane")
rectangle(slot, -3, 1, 3, 4)
pocket = solid("PartDesign::Pocket", "Slot")
pocket.Profile = slot
pocket.Type = "ThroughAll"
pocket.ReferenceAxis = (origin["X_Axis"], [""])
slot.Visibility = False
before = volume(pad)
if abs(volume(pocket) - before) < 1e-6:
    pocket.Reversed = True
assert volume(pocket) < before - 1, "the slot cuts"
log("slot", volume(pocket))

mirrored = solid("PartDesign::Mirrored", "Mirrored")
mirrored.Originals = [pocket]
mirrored.MirrorPlane = (origin["YZ_Plane"], [""])
log("mirrored", volume(mirrored))

top = sketch("PolarSketch", "XY_Plane")
top.AttachmentOffset = FreeCAD.Placement(V(0, 0, 5), FreeCAD.Rotation())
top.addGeometry(Part.Circle(V(7, 7, 0), V(0, 0, 1), 1), False)
polar_hole = solid("PartDesign::Pocket", "PolarHole")
polar_hole.Profile = top
polar_hole.Length = 2
top.Visibility = False
log("polar hole", volume(polar_hole))
polar = solid("PartDesign::PolarPattern", "PolarPattern")
polar.Originals = [polar_hole]
polar.Axis = (origin["Z_Axis"], [""])
polar.Angle = 360
polar.Occurrences = 4
log("polar", volume(polar))

row = sketch("RowSketch", "XY_Plane")
row.AttachmentOffset = FreeCAD.Placement(V(0, 0, 5), FreeCAD.Rotation())
row.addGeometry(Part.Circle(V(-25, 10, 0), V(0, 0, 1), 1), False)
row_hole = solid("PartDesign::Pocket", "RowHole")
row_hole.Profile = row
row_hole.Length = 1
row.Visibility = False
log("row hole", volume(row_hole))
linear = solid("PartDesign::LinearPattern", "LinearPattern")
linear.Originals = [row_hole]
linear.Direction = (origin["X_Axis"], [""])
linear.Length = 10
linear.Occurrences = 3
log("linear", volume(linear))

binder = body.newObject("PartDesign::ShapeBinder", "ProfileBinder")
binder.Support = [(profile, "")]
doc.recompute()

corners = []
for i, e in enumerate(linear.Shape.Edges):
    a, b = e.Vertexes[0].Point, e.Vertexes[-1].Point
    if abs(a.x - b.x) < 1e-9 and abs(a.y - b.y) < 1e-9 and abs(abs(a.x) - 30) < 1e-9 and abs(abs(a.y) - 15) < 1e-9:
        corners.append("Edge%d" % (i + 1))
assert len(corners) == 4, corners
fillet = solid("PartDesign::Fillet", "Fillet")
fillet.Base = (linear, corners)
fillet.Radius = 2
log("fillet", volume(fillet))

doc.saveAs(os.path.join(os.getcwd(), "plate.FCStd"))
log("saved", fillet.Shape.Volume, fillet.Shape.BoundBox)
FreeCADGui.getMainWindow().close()
