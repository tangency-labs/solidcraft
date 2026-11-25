# A box bottom's round keeps a trim that ends an ulp past its surface's domain

- **Unit:** `blend.fillet_executive_p3` (the fillet executive's old path, cross-section 1),
  seen through `topology.face_p1`'s `shrink_geometry`.
- **Input:** a 25 x 25 x 10 box, its four bottom edges rounded r 2 with `fillet_edges(k, b,
  &bottom, 2.0, 1, 1, 1.0)` on the default kernel with `face_swap_uv_transposes_trims`
  (WR-729): the case "filleted box bottom" of
  `crates/solidcore/tests/issue_2479_pcurve_ends_in_domain.rs`.
- **What happens:** one round's face (an `SmCone` cylinder, face 14) has a UV trim curve, edge
  use 119, whose start evaluates to `u = 1.0000000000000002`, one ulp past the surface's
  domain end `u = 1`. The fillet makes that trim. Since the face shrink keeps the trim curves
  where the trim keeps the surface's map (WR-700, the default since the flag was removed), the
  face keeps it as made. Before, the shrink removed every trim and projected the edges again,
  so the scan saw no overshoot, and the trims were fits instead of the exact curves.
- **Measured** (scratch run of the case at main 7efbd5090 and at the flag removal's head
  1d8f57b07): `check_brep` at level 2 reports no failure on either side; only the domain scan
  of `pcurve_domain_scan.rs` reports the end, at the head.
- **Where the fix belongs:** the fillet executive's trim construction, the producer class of
  the open issue tangency-labs/solidcore#2479 (pcurves ending an ulp past their face's
  domain), not the shrink: clamping a kept trim would hide the producer. The case is in that
  test's `SURVEY_ONLY` list next to the old chamfer, whose faces end an ulp past `v = 1` for
  the same reason, until the producer ends on the boundary; then it goes back to the asserting
  scan.
