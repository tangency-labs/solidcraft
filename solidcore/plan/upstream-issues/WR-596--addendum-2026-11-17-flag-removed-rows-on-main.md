Updates: WR-596 (`WR-596--createbox-drops-every-face-and-stitch-status.md`)

#### WR-596 (update, 2026-11-17). The removal's rows, re-measured on the current main

The 2026-11-12 addendum (`WR-596--addendum-2026-11-12-flag-removed.md`) counted 23 moved rows. Blessed again against main at 2745087ce (tangency-labs/solidcraft#18402, which carries that change), `box_face_failures_propagate`'s removal moves 27 rows, each from a success to a refusal. Main's body at each row was measured (`check_brep` level 2, face count, `is_manifold_solid`):

- `primitive.api_primitives`, 22 rows: the 18 of that addendum, and `gen0179`, `gen0260`, `gen0356`, `gen0756`. MEASURED for the four: main returned a 2-face body that fails level 2 (`SmBrep::Test #8 Bad Infinite Region`).
  - Of the 18, MEASURED: 11 returned 0 faces (the 1e6-scale boxes, the 1e12, 1e95, 1e200 and 1e294 origins, `gen0521`, `gen0711`, `gen1014`); `box_l_above_effzero`, `gen0025`, `gen0165`, `gen0627` and `wr596_box_smaller_than_tolerance` fail level 2 (bad loop orientation, zero-area loops); `box_l_huge` has edge gaps, 5 faces and is not manifold; `gen0939` is a 1-face open sheet where a closed box was asked.
- `offset.offset_executive_p2`, 4 rows, and `blend.fillet_executive_p2` `set_params_short_box_regression`: as in that addendum (the fixture's box is refused at construction; main built it empty or with the wrong edge count).

Every `api_primitives` box row is asserted by `goldens` `primitive_api_primitives::moved_rows::each_moved_box_row_is_refused_for_its_reason` (typed refusal site and reason). 21 rows are PROVEN by a closed form, and one is a kernel limit:
- `Collapses` (the 1e95, 1e200 and 1e294 origins, and `gen0711` at z = 1e18): origin + side == origin in f64, so two opposite faces are one plane.
- `BelowTolerance` (sides of 1e-6 and below, down to 2e-12): the shortest side is within the new B-rep's tolerance of zero, so each edge along it has one vertex.
- `SpacingAboveTolerance` (`box_origin_1e12_size_0p5_control`, `box_l_huge`, `gen0939`): the f64 spacing at the largest coordinate the box reaches exceeds the B-rep's tolerance, so no point out there can be placed within the tolerance of where it belongs. ulp(1e12) = 1.2e-4 and ulp(1e18) = 128, against a tolerance of 1e-5.
- `KernelLimit` (`box_at_1e6_size_1e_3_refused`): no closed form. At (1e6, 1e6, 1e6) the f64 spacing is 1.2e-10, and the 3e-3 cube builds there: `a_box_at_1e6_builds_from_3e_3` asserts that the 1e-3 cube is refused and that the 3e-3 cube is six faces and level-2 clean. The refusal stands because main's answer was the empty body (0 faces), which is no box either; it is not claimed that the 1e-3 box cannot exist.

None of these rows built a valid body on main: each was empty, failed level 2, or was a 1-face open sheet where a box was asked.

The offset rows use the same two boxes as `box_1e6_size_1e_6_refused` (`BelowTolerance`) and `box_at_1e6_size_1e_3_refused` (the kernel limit above). On main their source box was empty (0, 0, 0), and they are asserted by `offset_offset_executive_p2::tests::empty_boxes_are_recorded_unless_a_face_is_selected`.
