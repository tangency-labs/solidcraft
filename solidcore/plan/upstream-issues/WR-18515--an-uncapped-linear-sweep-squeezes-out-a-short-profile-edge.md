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
- **Trigger:** a 10 x 10 loop whose top side ends in a segment 0.00017 long, extruded 1.2 with
  no caps: 5 faces, 13 edges, 8 vertices; the check fails `SmTrimmingTools` tests 0 (Edge/Edge
  Gap, offset 0.00017, tolerance 2e-5) and 1 (UVEdge/UVEdge Gap, offset 1). Segments of 3e-5,
  1e-4 and 3.9e-4 fail the same way; 4.1e-4 and longer build 15 edges and 10 vertices.
- **How found:** a user's traced outline (SolidCraft #18515, Surface Extrude), reduced.
- **Confirmed:** yes, in the port, which keeps upstream's two passes unchanged:
  `sweep::extrude::tests::an_uncapped_profile_with_a_short_segment_keeps_every_wall` fails
  without the fix. With the second pass switched off the same sweeps build 15 edges and 10
  vertices and pass the level-2 check. The pinned C++ was not run.
- **solidcore:** `create_linear_sweep` without end caps stitches with
  `stitch_and_orient_open`, the first pass only. A capped sweep keeps both passes: its caps
  close every profile edge in the first pass, so a lamina edge left after it is a gap.
- **Not fixed here:** a segment shorter than twice the body's tolerance (1e-5 at 1e-5) is
  squeezed by the first pass too; that sheet fails the loop orientation tests (3, 4) and the
  capped extrude is refused, with and without this fix.
