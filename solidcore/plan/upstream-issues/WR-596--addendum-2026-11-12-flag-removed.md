Updates: WR-596 (`WR-596--createbox-drops-every-face-and-stitch-status.md`)

#### WR-596 (update, 2026-11-12). `box_face_failures_propagate` is removed

`PrimitiveCreation::create_box` now always returns the status of each plane, face and its `StitchAndOrient`, and makes no face on a side of zero extent, as the fork's `AddBoxFace` does. Upstream drops those statuses and returns success with whatever faces were made. Propagation is the only path now, and the flag is gone: the field, the old ignore path in `box_faces`, SolidCraft's preset line, its entry in `fixes.json` and in issue_2260's list of SolidCraft's flags, and every test setting of it are removed (tangency-labs/solidcraft#15350, a first step before a geometric `Surface::is_degenerate_curve`, which judges more box faces degenerate).

- **Results** (every golden blessed on main and with the change; 23 rows move, each from a success to a refusal):
  - `primitive.api_primitives`, 18 rows:
    - 9 came back as a success with an empty body (0 faces, 0 edges, 0 vertices) and now refuse. `box_1e6_size_1e_6_refused`, `box_at_1e6_size_1e_3_refused`, `box_origin_1e12_size_0p5_control`, `gen0521`, `gen0711` and `gen1014` refuse with 1001. `box_origin_1e95_size_0p5`, `box_origin_x_1e95_size_0p5`, `box_origin_1e200_size_0p5`, `box_origin_1e294_size_1em3` and `box_origin_x_1e294_size_0p5` refuse with 1007, a plane that cannot be made at that origin.
    - 7 came back as a success with faces missing (a whole box's record is 361 values; these were 305, 343, 181 and 111): `box_l_above_effzero`, `box_l_huge`, `gen0025`, `gen0165`, `gen0627`, `gen0939` and `wr596_box_smaller_than_tolerance` (the 4-face body of the fuzz case). Each now refuses.
    - The boxes at 1e6 scale (size 1e-3 or 1e-6 at 1e6) already failed to make their faces on main, which returned an empty body. The refusal states that failure; it does not create one.
  - `offset.offset_executive_p2`, 4 rows: `review_box_{tiny,translated}_{source,foreign}_null` record the box constructor's refusal as constructor `rejected`, status 1001, where they built the empty box and recorded `api_shell`'s 1007.
  - `blend.fillet_executive_p2` `set_params_short_box_regression`: its fixture's box now fails to build with 1001 (`brep_build_status_1001`); before, it built with the wrong edge count (`box_edge_count`).
- **Tests:** `tests/wr_596_tiny_box_fails.rs` (`wr_596_tiny_box_is_sm_err`, `wr_596_an_ordinary_box_has_six_faces`); `goldens` `offset_offset_executive_p2::tests::empty_boxes_are_recorded_unless_a_face_is_selected` (the 4 refused boxes are constructor `rejected` with 1001, the other null selections 1007).
