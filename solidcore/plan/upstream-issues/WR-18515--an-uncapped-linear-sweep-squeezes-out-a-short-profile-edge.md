#### WR-18515. An uncapped linear sweep squeezes out a short profile edge, so the sheet fails the loop gap tests
- **Severity:** Wrong result (a sheet the check refuses, where every wall exists in exact geometry)
- **Area:** Primitives (curve linear sweep), stitching
- **Location:** `source/SMLib/src/SmPrimitiveCreation.cpp`, the curve `CreateLinearSweep`, which ends
  with `StitchAndOrient`; `StitchAndOrient` runs a second `StitchFaces` pass at twenty times the
  first pass's tolerance when the first leaves more than one lamina edge, and `StitchFaces` sets
  `SqueezeSmallEdges` (solidcore `primitive/primitive_creation/p1/primitives.rs`
  `create_linear_sweep`, `topology/brep/p5.rs` `stitch_and_orient_searching`,
  `heal/stitch/p2.rs` `do_stitching_body`).
- **What happens:** without end caps the sweep builds one wall per profile curve. The walls meet
  exactly at the profile's shared curve ends, so the first pass (twice the body's tolerance,
  2e-5 at 1e-5) joins every side edge. The profile's two copies, the bottom and top edges of
  the walls, stay lamina: they are the sheet's open boundary. So the second pass always runs,
  at 4e-4, and squeezes out every lamina edge shorter than that. For a wall swept from a curve
  between 2e-5 and 4e-4 long, both of its profile edges go: its loop keeps its two side edges
  only, with a gap of the curve's length in 3D and of the wall's whole u range in UV.

  ![The walls of an uncapped extrude unrolled side by side: wall A, the short wall S (drawn wide) and wall B. Left, on main: the second pass's reach covers S's bottom edge, and S's bottom and top edges are squeezed out, so S keeps only its two side edges. Right, with the first pass only: every wall keeps its bottom, top and side edges.](WR-18515--squeezed-short-wall.svg)

  The second pass removes the short wall's profile edges; the first pass alone keeps them.

- **Trigger:** a 10 x 10 loop whose top side ends in a segment 0.00017 long, extruded 1.2 with
  no caps: 5 faces, 13 edges, 8 vertices; the check fails `SmTrimmingTools` tests 0 (Edge/Edge
  Gap, offset 0.00017, tolerance 2e-5) and 1 (UVEdge/UVEdge Gap, offset 1). Segments of 3e-5,
  1e-4 and 3.9e-4 fail the same way; 4.1e-4 and longer build 15 edges and 10 vertices.
- **How found:** a user's traced outline (SolidCraft #18515, Surface Extrude), reduced.
- **Confirmed:** yes, in the port, which keeps upstream's two passes unchanged:
  `sweep::extrude::tests::an_uncapped_profile_with_a_short_segment_keeps_every_wall` fails
  without the fix (13 edges, 8 vertices for a 3e-5 segment). With the second pass switched
  off the same sweeps build 15 edges and 10 vertices and pass the level-2 check. The pinned
  C++ was not run.
- **Same cause:** solidcore's sweep along lines and arcs (`sweep/segments.rs`,
  `sweep_along_segments`, no upstream counterpart) copies the pieces' walls into one body
  and ends with `stitch_and_orient`. Uncapped, the profile's copies at both ends stay
  lamina, and the second pass squeezes their short edges again:
  `sweep::segments::tests::an_uncapped_profile_with_a_short_segment_keeps_every_wall`
  fails without the fix.
- **A gap in the profile:** an uncapped square whose second side starts 3e-5 to 3.9e-4 off
  the first side's end was glued by the second pass without its tolerances covering the gap;
  the check refused the sheet (Bad Gap tests 0 to 4, Bad Edge loc 13). With the first pass
  only, the two walls stay apart along an open slit, and the check accepts the sheet
  (4 faces, 13 edges, 10 vertices). The capped solid is the same with and without the fix.
- **solidcore:** `create_linear_sweep` without end caps, and `sweep_along_segments` without
  caps, stitch with `stitch_and_orient_open`, the first pass only. A capped sweep keeps both
  passes: its caps close every profile edge in the first pass, so a lamina edge left after
  it is a gap.
- **Not fixed here:** a segment shorter than twice the body's tolerance (1e-5 at 1e-5) is
  squeezed by the first pass too; that sheet fails the loop orientation tests (3, 4) and the
  capped extrude is refused, with and without this fix.
