# Writes grid.FCStd and scaled.FCStd, two PartDesign bodies whose holes are repeated by
# MultiTransforms.
#
# grid.FCStd, a body called "Grid": an 80 x 50 x 5 mm plate.
# - A 3 mm hole through all at (-36, -18), repeated by "GridHoles", a MultiTransform of three steps:
#   3 along X 6 mm apart, then 3 along Y 6 mm apart, then mirrored across the YZ plane (18 holes).
# - A 2 mm hole 2 mm deep at (9, 3), repeated by "Spokes", a MultiTransform of two steps: 3 times
#   about the Z axis, then 2 along the Y axis 12 mm apart (6 holes). Turning first and moving second
#   puts the holes elsewhere than the other order would.
#
# scaled.FCStd, a body called "Scaled": a 40 x 20 x 5 mm plate with a 2 mm hole 2 mm deep at (-15, 0),
# repeated by a MultiTransform of a step along X (2 holes) and a Scaled step (2 sizes), which
# SolidCraft does not rebuild.
#
# Run from this directory with FreeCAD's command line program, for example
# `freecadcmd make_grid_fcstd.py` (made with FreeCAD 1.1 for Windows, run from WSL as
# `"/mnt/c/Program Files/FreeCAD 1.1/bin/freecadcmd.exe" make_grid_fcstd.py`). FreeCAD's volumes are
# printed at the end and recorded in `../tests.rs`.
import os

import FreeCAD
import Part

V = FreeCAD.Vector
HERE = os.getcwd()


def new_body(doc, name):
    body = doc.addObject("PartDesign::Body", name)
    return body, {f.Role: f for f in body.Origin.OriginFeatures}


def sketch(body, origin, name, z=0.0):
    sk = body.newObject("Sketcher::SketchObject", name)
    sk.AttachmentSupport = (origin["XY_Plane"], [""])
    sk.MapMode = "FlatFace"
    sk.AttachmentOffset = FreeCAD.Placement(V(0, 0, z), FreeCAD.Rotation())
    return sk


def rectangle(sk, x0, y0, x1, y1):
    c = [V(x0, y0, 0), V(x1, y0, 0), V(x1, y1, 0), V(x0, y1, 0)]
    for i in range(4):
        sk.addGeometry(Part.LineSegment(c[i], c[(i + 1) % 4]), False)


def solid(body, kind, name):
    # Each solid feature builds on the one before: it becomes the body's tip.
    o = body.newObject(kind, name)
    body.Tip = o
    return o


def valid(doc, o):
    doc.recompute()
    assert o.isValid(), o.Name
    assert not o.Shape.isNull(), o.Name
    return o.Shape


def plate(doc, body, origin, x, y):
    profile = sketch(body, origin, "Profile")
    rectangle(profile, -x / 2, -y / 2, x / 2, y / 2)
    pad = solid(body, "PartDesign::Pad", "Pad")
    pad.Profile = profile
    pad.Length = 5
    return valid(doc, pad)


def hole(doc, body, origin, name, at, radius, depth):
    sk = sketch(body, origin, name + "Sketch", 5)
    sk.addGeometry(Part.Circle(V(at[0], at[1], 0), V(0, 0, 1), radius), False)
    pocket = solid(body, "PartDesign::Pocket", name)
    pocket.Profile = sk
    if depth is None:
        pocket.Type = "ThroughAll"
    else:
        pocket.Length = depth
    return pocket


def multi(body, name, originals):
    mt = solid(body, "PartDesign::MultiTransform", name)
    mt.Originals = originals
    return mt


def step(body, mt, kind, name):
    # A MultiTransform's steps are in its body, after it, but the MultiTransform stays the tip.
    o = body.newObject(kind, name)
    body.Tip = mt
    mt.Transformations = mt.Transformations + [o]
    return o


def report(name, shape):
    c = shape.Solids[0].CenterOfMass if shape.Solids else None
    print(name, "volume", repr(shape.Volume), "centre", c, "box", shape.BoundBox)


# grid.FCStd
doc = FreeCAD.newDocument("Grid")
body, origin = new_body(doc, "Grid")
plate(doc, body, origin, 80, 50)

grid_hole = hole(doc, body, origin, "GridHole", (-36, -18), 1.5, None)
grid = multi(body, "GridHoles", [grid_hole])
along_x = step(body, grid, "PartDesign::LinearPattern", "AlongX")
along_x.Direction = (origin["X_Axis"], [""])
along_x.Length = 12
along_x.Occurrences = 3
along_y = step(body, grid, "PartDesign::LinearPattern", "AlongY")
along_y.Direction = (origin["Y_Axis"], [""])
along_y.Length = 12
along_y.Occurrences = 3
across = step(body, grid, "PartDesign::Mirrored", "AcrossYZ")
across.MirrorPlane = (origin["YZ_Plane"], [""])
valid(doc, grid)

spoke = hole(doc, body, origin, "Spoke", (9, 3), 1, 2)
spokes = multi(body, "Spokes", [spoke])
turn = step(body, spokes, "PartDesign::PolarPattern", "AboutZ")
turn.Axis = (origin["Z_Axis"], [""])
turn.Angle = 360
turn.Occurrences = 3
move = step(body, spokes, "PartDesign::LinearPattern", "UpY")
move.Direction = (origin["Y_Axis"], [""])
move.Length = 12
move.Occurrences = 2
grid_shape = valid(doc, spokes)
doc.saveAs(os.path.join(HERE, "grid.FCStd"))
report("grid", grid_shape)

# scaled.FCStd
doc = FreeCAD.newDocument("Scaled")
body, origin = new_body(doc, "Scaled")
plate(doc, body, origin, 40, 20)
seed = hole(doc, body, origin, "Seed", (-15, 0), 1, 2)
scaled = multi(body, "Growing", [seed])
row = step(body, scaled, "PartDesign::LinearPattern", "Row")
row.Direction = (origin["X_Axis"], [""])
row.Length = 20
row.Occurrences = 2
grow = step(body, scaled, "PartDesign::Scaled", "Grow")
grow.Factor = 2
grow.Occurrences = 2
scaled_shape = valid(doc, scaled)
doc.saveAs(os.path.join(HERE, "scaled.FCStd"))
report("scaled", scaled_shape)
