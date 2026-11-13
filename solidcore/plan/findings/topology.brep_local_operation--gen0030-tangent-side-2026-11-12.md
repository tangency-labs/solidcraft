# gen0030: a side tilted into its round is built, not refused (2026-11-12)

Evidence for the one `topology.brep_local_operation` golden row that tangency-labs/solidcraft#13718
moves. It does not justify other rows.

## The case

`gen0030` reads `upstream/TestFiles/pt_TestFiles/box_with_large_fillet.smb` (a 10 mm box whose
edge at x = z = 10 is rounded r 8 about the axis x = z = 2), without rebuilding its trims, and
replaces three faces with planes (`p + offset·n`, normal `n + tilt`, from each face's middle):

- face 0 (z = 0, n = (0, 0, −1)): offset 0, no tilt (its own plane);
- face 4 (y = 10, p = (5, 10, 5), n = (0, 1, 0)): offset `-0.06811719631876167`, tilt
  `[0.08793954552342825,0.07961790888854042,-0.2941607901132638]`;
- face 3 (x = 10, p = (10, 5, 5), n = (1, 0, 0)): offset `-0.4721612964504289`, tilt
  `[-0.06661980589432345,-0.1406187786332792,-0.27482081400878505]`.

Face 3 is tangent to the round along z = 2. Moved in and tilted, its plane cuts the r 8 cylinder:
the side and the round cross along their old tangent edge.

## Before and after

- Before (main 9e805b7bdc): refused, `LoopGapUncertified { face: 3 }`, the input put back.
- After: built, 7 faces, 15 edges, 10 vertices (as the input). The check at level 2 reports
  nothing, the body is a manifold solid, and its volume is certified in [795.35, 796.67].

## Independent checks

- **Volume.** The body is the region x ≥ 0, y ≥ 0, z ∈ [0, 10] on the inner side of the two new
  planes, and inside the cylinder (x − 2)² + (z − 2)² ≤ 64 wherever x > 2 and z > 2. Integrating
  its exact y-extent over an (x, z) midpoint grid (no kernel value enters) gives 796.0148,
  796.0085, 796.0068 and 796.0084 at 1000², 2000², 4000² and 8000² cells: 796.008 ± 0.003, inside
  the certified enclosure.
- **Vertices.** Every vertex lies within 9.3e-7 of each of its three faces' new surfaces (point
  drops onto the planes and the cylinder); vertex tolerances are 1e-5 to 1.07e-5, except the
  vertex (2, 0, 10), where the round meets the top on the unmoved side y = 0, which keeps the
  input's 2.37e-3.
