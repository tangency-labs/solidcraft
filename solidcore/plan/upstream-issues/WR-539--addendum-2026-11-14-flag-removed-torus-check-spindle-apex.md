Updates: WR-539

#### WR-539 (update, 2026-11-14). `torus_check_spindle_apex` is removed

`SmTorus::AssertValid` always tests a spindle torus's apexes `V = ±(90 + asin(R/r))` against its V interval and reports an apex inside it as test 5, "Self Intersection" (the fork's tangency-labs/smlib@daba678). Upstream's silent `acos` test of the U interval is gone, with the field, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** 47 rows of `surface.surface_assert_valid` move, all spindle tori (R < r; one, `wr552_horn_torus_face_m0`, a horn torus a few ulps short of R = r).
  - 38 gain test 5. In each, one or both apexes lie strictly inside the V interval, at least 3.13 degrees from its nearer bound. Of these, 33 were already invalid with no report, because upstream's branch cleared the result silently. The other 5 (`gen0326`, `gen0345`, `gen0687`, `gen1214`, `gen1363`) were reported valid only because their U interval missed upstream's `acos` range.
  - 9 become valid with no report change (`gen0271`, `gen0343`, `gen0485`, `gen1172`, `gen1280`, `gen1414`, `gen1427`, `gen1455`, `wr552_horn_torus_face_m0`). Neither apex is inside the V interval: the face is one sheet of the torus, or it ends at an apex. Upstream failed them because their U interval met the `acos` range.
- **Tests:** `wr_539_spindle_torus_reports_an_apex_inside` in `surface/surface/assert_valid.rs`; `crates/solidcore/tests/issue_1672_torus_minus_z.rs`.
