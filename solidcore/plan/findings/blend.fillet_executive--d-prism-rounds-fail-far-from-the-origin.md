# blend.fillet_executive: the D prism's three rounds fail far from the origin, with or without the curved Setback (2026-11-03)

- **Unit:** `blend.fillet_executive` (the session's `finish`). Found in PR #2366 (#2343) while checking the curved Setback far from the origin. It is not caused by that PR: main's corner path fails the same way.
- **Inputs:** the D prism of `crates/solidcore/tests/issue_2343_curved_setback.rs`, built at an offset o: `d_prism(k, o, o, 10, 4, o, 10)`, every fix on. Its top chord, its upright at the corner and its top arc are rounded 2, 2.5 and 3 together (`round_each`, as `d_prism_corner_far_from_the_origin_is_a_setback` does).
- **Result:** `do_filleting` fails with "4 Fillet Surface Computation Failure" (`Upstream { operation: "blend::fillet_executive::session::finish", code: 1001 }`).
  - It fails at o = 1.5e3, 2.5e3, 3e3, 4e3, 5e3, 6e3, 7e3, 8e3, 9e3, 1e4, 1.2e4, 2e4, -4e3 and -1e4.
  - It builds at o = 0, 1e3, 2e3, -1e3 and -2e3.
  - With `fillet_setback_beside_curved_faces` off (the N-by-N convex corner, main's path) and on (the curved Setback), the outcome is the same at every offset where both were run (0, 1.5e3, 2e3, 3e3 to 2e4, -4e3, -1e4). 1e3, 2.5e3, -1e3 and -2e3 were run with it on only.
- **Also** (at o = 0, 1.5e3, 4e3 and 1e4): the top arc rounded 3 alone fails the same way at each, the origin included. The chord alone (2) and the upright alone (2.5) build at each. The chord and the arc together build at 0, 1.5e3 and 4e3 and fail at 1e4.
- **Expected:** a rigid translation of the part does not change whether its rounds build.
- **Suspected cause:** not investigated. The lone arc's failure at the origin suggests the round along the arc where it ends on the faces at its open ends; the offsets suggest an absolute tolerance or a rounding floor somewhere in the session's surface computation.
- **State:** open.
