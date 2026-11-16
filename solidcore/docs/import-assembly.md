<!-- SPDX-License-Identifier: Apache-2.0 -->

# Import assembly

This page describes the stage that turns the faces, edges and vertices an importer has read into a finished B-rep record (schema 4, `solidcore_record::brep`). It is `solidcore_step::assembly`. The STEP reader uses it, and any other reader of a B-rep file format can use it too. The stage keeps one implementation of the work every reader needs: two readers that hand it the same geometry get the same record.

## What a reader gives

A reader creates an `Assembly` with a tolerance (mm) and, optionally, a `Trims`: the kernel that makes the pcurves which have no closed form. It then adds the pieces of one body:

- **Vertices** (`add_vertex`): a point each.
- **Edges**, each on a record curve between two vertices:
  - `add_edge` takes the edge's interval on the curve, from min to max. It keeps whichever way round puts the vertices nearer the curve's points at the interval's ends, and notes an edge it turns. Each vertex's tolerance grows to cover its gap; a vertex too far off refuses the edge.
- **Faces** (`add_face`), each on a record surface with its bounds. A bound is a closed chain of edge uses (`Bound::Edges`) or one vertex (`Bound::Vertex`: a pole or an apex). Bounds are relative to the surface's natural normal: seen from that normal, an outer loop runs counter-clockwise. `forward` says whether the face's side is that normal. `origin` is the reader's name for the face; refusals name the face by it, and the record keeps it (`Solid::face_origin`).
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

The STEP reader reads its own surfaces of linear extrusion and of revolution, and its cones, by the same code.


## What `finish` does

`finish(item, name, shells, kind)` takes each shell as a list of faces with the side the shell uses each on, and what they make (`Kind`: a solid, a solid with voids, a surface model, or a solid with faces left out). In order, it:

1. Merges edges written twice: two edges on the same vertices and the same curve over the same interval, each used by one face.
2. Completes faces that cover all of a closed surface:
   - A sphere face bounded only by its pole vertices gets a meridian for its seam.
   - A face bounded only by a slit gets its seams.
3. Splits edges that pass through a face's pole, and adds seam edges to faces that wind around a periodic surface without one.
4. Turns round faces' frames so that each face's NURBS form covers it once.
5. Gives each edge its label and NURBS form.
6. Makes every use's pcurve:
   - from the closed form, where the geometry has one;
   - from the trims, otherwise;
   - from the edge's own angle and height, on a face that winds more than one turn about its axis.
7. Picks the outer loops, and turns over a face whose outer loop runs clockwise about its side.
8. Raises edge and vertex tolerances to the gaps the geometry measurably has.
9. Turns inside out a void that encloses volume of the outer shell's sign.
10. Builds the regions and puts everything in the record's canonical order.
11. Checks the record's structure (`record_check::validate`) and its geometry against its topology (`record_check::agreement`).

The result is a `Solid`: the record, each face's origin, whether it is a sheet, the notes on what was repaired, and the faces turned over.

## What it refuses

A body is refused, with the face named, when:

- a vertex or edge lies off its curve or surface by more than 1 mm;
- a loop is not closed in its surface's parameters and cannot be given a seam;
- a face winds more than one turn about its axis on a surface that is not a cylinder or a cone;
- the pcurves of a use cannot be made;
- the finished record fails its checks.

What a reader does with a refusal is up to it: the STEP reader may read the body's faces one by one instead, and the CADIR reader refuses that body region and keeps the others.

## Where it is

The stage is in `solidcore-step`, under the transitional entry that lets SolidCraft use that crate until STEP import becomes kernel primitives under an ops recipe (RFC 0003, 1.6). Its geometry is the STEP reader's, moved without change, so moving it to its final layers is one job for both readers.
