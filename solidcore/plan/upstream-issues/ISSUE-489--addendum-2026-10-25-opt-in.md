#### Updates: Issue #489 (2026-10-25). Fixed on main, on by default

- The Rust port already fixes #489 on main (446a6acb1): `FaceFaceIntersect` gives
  `MakeEdgeInFace` a copy of each classification's current UV curve, so the edge's trims
  are the intersection's own parameter curves instead of a projection fit. This is on by
  default, as the fix predates the opt-in policy; a fix already on main keeps its
  default.
- **Code:** `copy_intersection_edge_curves` and `make_edge_for_interval` in
  `crates/solidcore/src/boolean/topology_intersector/p1.rs` (header Deviations, "Intersection
  trims").
- **Tests:** `intersection_edge_copies_each_faces_uv_with_its_parameterisation` and the
  other `copy_intersection_edge_curves` tests in that file.
- The end-to-end gap reduction (2–4e-6 on the #474 thread) is still not certified (see the
  original entry).
- Fixes tangency-labs/solidcore#489.
