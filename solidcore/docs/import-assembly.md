<!-- SPDX-License-Identifier: Apache-2.0 -->

# Import assembly

This page describes the stage that turns the faces, edges and vertices an importer has read into a finished B-rep record (schema 4, `solidcore_record::brep`). It is `solidcore_step::assembly`. The STEP reader uses it, and any other reader of a B-rep file format can use it too. The CADIR reader (`solidcore-cadir`) is the second one. The stage keeps one implementation of the work every reader needs: two readers that hand it the same geometry get the same record.

## What a reader gives

A reader creates an `Assembly` with a tolerance (mm) and, optionally, a `Trims`: the kernel that makes the pcurves which have no closed form. It then adds the pieces of one body:

- **Vertices** (`add_vertex`): a point each.
- **Edges**, each on a record curve between two vertices:
  - `add_edge` takes the edge's interval on the curve, from min to max. Its `origin` is the reader's name for the edge, which the record keeps (`Solid::edge_origin`; a piece of an edge the stage splits keeps its edge's, and an edge the stage makes, a seam, has none). It keeps whichever way round puts the vertices nearer the curve's points at the interval's ends, and notes an edge it turns. Each vertex's tolerance grows to cover its gap; a vertex too far off refuses the edge.
  - `add_edge_on` is for a producer that bounds an edge by its vertices alone. It finds the interval where the vertices lie on the curve, and starts a closed curve at its vertex.
  - `periodic_edge`, `parabola_curve` and `hyperbola_curve` give the record curve and interval for an edge on a periodic spline, a parabola or a hyperbola.
- **Faces** (`add_face`), each on a record surface with its bounds. A bound is a closed chain of edge uses (`Bound::Edges`) or one vertex (`Bound::Vertex`: a pole or an apex). Bounds are relative to the surface's natural normal: seen from that normal, an outer loop runs counter-clockwise. `forward` says whether the face's side is that normal. `origin` is the reader's name for the face; refusals name the face by it, and the record keeps it (`Solid::face_origin`).
- **Faces with the producer's pcurves** (`add_face_with_pcurves`): as `add_face`, with the pcurve of each use the producer holds, in the surface's own (u, v), on the edge's interval. A reader of a body in memory gives these (SolidCraft's exact exchange between kernels). A use with a closed form keeps it, and one without takes the given pcurve instead of one the `Trims` fit; nothing given is used on a face whose surface has a NURBS form of its own in the record (its pcurves are on that form), for an edge on a conic (its pcurves run in its NURBS form's parameter), on a bound the stage joins, splits or turns, or on a surface whose parameters it moves (a closed patch whose seam it moves). Each edge's tolerance covers its given pcurves' gaps as it covers a file's (step 9), and the record's check holds them to their edges.
- **Spline curves and surfaces** come from full knot vectors through `spline_curve` and `spline_surface`. These refuse knots that are not finite or decrease, an empty domain, coordinates or weighted coordinates that are not finite, and weights that are not positive. Unclamped knots are clamped exactly, by knot insertion.

Everything is in millimetres and radians.

## Surfaces that depend on their face

Some surfaces have no single record form: what the record holds depends on the face on them. A reader gives such a surface by its definition, a `SweptSurface`, and asks `swept_surface(surface, bounds)` for the record surface once the face's edges and vertices are added. It returns the record surface and whether its natural normal is opposite the definition's own; the reader then adds the face on that surface with `add_face`, its bounds and side taken relative to the record's natural normal, as for any other surface. The bounds are given as the reader has them, either way round: only the points along them are used.

- **A linear sweep** (`LinearSweep`): a record curve moved along a direction, `profile(s) + t direction`. Its own normal is the profile's tangent × the direction.
- **A revolution** (`Revolution`): a record curve turned about an axis (a point and a direction), the angle counter-clockwise about the direction. Its own normal is ∂angle × ∂profile.
- **A cone** (`Cone`): the record's circular cone (a frame, the radius at its origin, the half angle). Its own normal is the record cone's.

What the record holds:

- A line, circle or ellipse profile is bounded where the face needs it: over the parameters of the face's boundary points with a margin, or, on a closed profile, across the widest gap the face's loops leave (so the profile's own start, which may lie inside the face, does not cut it), or the whole turn from the face's seam when a loop goes once round. A swept profile becomes an exact spline along it, degree 1 along the direction; a revolved one a revolution, stopped where the profile meets the axis. A spline or polyline profile is swept whole.
- A circle swept along its axis is the cylinder it is, and a circle revolved in a plane through the axis the ring torus it is.
- A cone that stays within the tolerance of the plane through its frame's origin across its axis, over the face's boundary points (and its apex, should the face hold the axis), is that plane: a cone a whisker short of a right angle, as some writers give a plane, has no area in its own parameters.

The STEP reader reads its own surfaces of linear extrusion and of revolution, and its cones, by the same code. The CADIR reader gives its cones this way, and its procedural linear sweeps, extrusions and revolutions that carry no solved form (STEP's swept surfaces and FreeCAD's, as cadmpeg reads them).


## What `finish` does

`finish(item, name, shells, kind)` takes each shell as a list of faces with the side the shell uses each on, and what they make (`Kind`: a solid, a solid with voids, a surface model, or a solid with faces left out). In order, it:

1. Merges edges written twice: two edges on the same vertices and the same curve over the same interval, each used by one face. Then, where the body is refused as given, leaves out each face the refusal names that is narrower than the tolerance (a sliver some writers leave where two faces meet at a corner, of no area in its own parameters) and reads it again: a face of one loop whose edges are two long ones, each used by one other face, lying within the tolerance of each other, and short ones no longer than the tolerance. The short edges' vertices are made one, the short edges go from every loop, and the two long edges are made one, so the faces either side of the sliver meet along it. A face that narrow which reads (a chamfer a few µm wide on a part written at a loose tolerance) is not named, and is kept.
2. Completes faces that cover all of a closed surface:
   - A sphere face bounded only by its pole vertices gets a meridian for its seam.
   - A face bounded only by a slit gets its seams.
3. Splits edges that pass through a face's pole, and adds seam edges to faces that wind around a periodic surface without one.
4. Turns round faces' frames so that each face's NURBS form covers it once.
5. Gives each edge its label and NURBS form.
6. Makes every use's pcurve:
   - from the closed form, where the geometry has one;
   - from the trims, otherwise, made within the face's NURBS form's domain, its surface's natural range, or (on a plane, which bounds nothing) its boundary's box with a quarter of its size round it;
   - from the edge's own angle and height, on a face that winds more than one turn about its axis.

   A round face's form ends on the quarter line its boundary runs past by up to the tolerance, and its pcurves are put on the form's range. Along a cylinder's or cone's height the form ends where the boundary does, and a pcurve end fitted a few µm past it is put on it where that moves the surface by no more than the tolerance. An edge that lies wholly past that line (a few µm of a circle beyond it) has no closed form on the form: it takes its closed form on the face's form not cut short, which has the same parameters, and that is put on the range as the others are.

   A face that lies across the seam of a spline patch closed on itself (its first and last control lines the same, as a patch whose ends meet within the tolerance is made when its face is added), without a seam edge, has no one set of pcurves on it: its loop would jump from one end of the range to the other. The patch is started instead in the middle of the widest gap the face's pcurves' control hulls (which enclose them) leave round it: that knot raised to the degree, the patch split there and its two pieces joined the other way round at the old seam. It is the same surface, so the face's pcurves are made again on it.

   A loop may jump across a spline face's range where a side of the patch a few µm long was written as one vertex (a corner patch whose two corners there the file made one). Where every point of that side lies within the vertex's tolerance of it, the loop runs along the side as at a pole, the vertex's tolerance raised so the side is within it from the loop's point there, as the record check measures it.

   A spline face's pcurves that end a little past the patch's knot range are put on it where that moves the surface within their edges' tolerance. Where it would move it more, the patch is continued instead to reach them: each end span's polynomial (homogeneous, so a rational patch keeps its shape) is taken over a longer interval, by at most 5% of that span as given. The patch is the same surface on its old range, so the pcurves stay as they are. A direction the patch closes in (its two boundary curves' control points within the tolerance and their weights in one ratio), or a weight that would not stay positive, is not continued.
7. Picks the outer loops, and turns over a face whose outer loop runs clockwise about its side.
8. Joins loops of one face that touch at one vertex into one loop through it (`assembly/touching.rs`): a hole whose rim touches the face's outer edge at one point, or two holes that touch. The kernel takes no face whose loops share a vertex ([validity-and-tolerances.md](validity-and-tolerances.md) §4), and the one loop bounds the same region with the same edges and vertices. Loops that touch at two vertices or as a ring (they cut the face in two), or three or more at one vertex, are left as written, and a note names the face and the point.
9. Raises edge and vertex tolerances to the gaps the geometry measurably has.
10. Orients an inconsistent closed solid shell by shared-edge parity: the two physical traversals of each shared edge must oppose. This determines relative face sides.
11. Turns inside out a void that encloses volume of the outer shell's sign when no shell needed parity repair.
12. Builds the regions and puts everything in the record's canonical order. After parity repair, certifies every shell's signed volume with the kernel's volume enclosure, turns the outer shell outward and the cavities inward, and restores canonical order.
13. Checks the record's structure (`record_check::validate`) and its geometry against its topology (`record_check::agreement`).

The result is a `Solid`: the record, each face's and edge's origin, whether it is a sheet, the notes on what was repaired, and the faces turned over.

## What it refuses

A body is refused when (face-local refusals name the face):

- a vertex or edge lies off its curve or surface by more than 1 mm;
- a loop is not closed in its surface's parameters and cannot be given a seam;
- a face winds more than one turn about its axis on a surface that is not a cylinder or a cone;
- the pcurves of a use cannot be made;
- a closed solid shell needs orientation repair but has a nonmanifold edge, disconnected faces, or contradictory shared-edge parity (a line where four faces meet that the file writes as two edges on the same vertices and curve counts as one edge, so a shell whose uses cancel along it needs no repair, however the file pairs the faces across them);
- a repaired shell's absolute orientation cannot be certified by its volume enclosure;
- the finished record fails its checks.

A refusal names the file's face it is about (`face #` and the face's `origin`): a refusal of pcurves names every face whose pcurves failed (up to 64, the first first), and a finished record whose geometry disagrees on a face names that face.

What a reader does with a refusal is up to it. Both readers read a refused solid again without the faces the refusal names, as an open surface of the faces that remain (`Kind::Open`; a void whose outer shell lost every face stays a void, `outer: false`), up to eight times, so a solid that cannot be closed still comes in as the faces that read. The STEP reader leaves out the first face named each time, after the faces that do not read alone; the CADIR reader leaves out every face named. The STEP reader also has SMLib stitch the faces that read alone where a refusal names no face. A refusal the readers cannot work around leaves out that body region and keeps the others.

## Where it is

The stage is in `solidcore-step`, under the transitional entry that lets SolidCraft use that crate until STEP import becomes kernel primitives under an ops recipe (RFC 0003, 1.6). Its geometry is the STEP reader's, moved without change, so moving it to its final layers is one job for every reader: the STEP and CADIR readers, and SolidCraft's exact exchange of bodies in memory.
