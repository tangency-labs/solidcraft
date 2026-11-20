Updates: WR-539

#### WR-539 (update, 2026-11-14). `torus_check_spindle_apex` is removed

`SmTorus::AssertValid` always tests a spindle torus's apexes `V = ±(90 + asin(R/r))` against its V interval and reports an apex inside it as test 5, "Self Intersection" (the fork's tangency-labs/smlib@daba678). Upstream's silent `acos` test of the U interval is gone, with the field, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** 47 rows of `surface.surface_assert_valid` move, all spindle tori (R < r; one, `wr552_horn_torus_face_m0`, a horn torus a few ulps short of R = r).
  - 38 gain test 5. In each, one or both apexes lie strictly inside the V interval, at least 3.13 degrees from its nearer bound. Of these, 33 were already invalid: 31 with no report, because their U interval met upstream's `acos` range and its branch cleared the result silently, and 2 (`gen0451`, U [0, 25.5] against [145.2, 214.8]; `gen1142`, U [0, 34.1] against [143.2, 216.8]) through other tests, their U interval missing that range. The other 5 (`gen0326`, `gen0345`, `gen0687`, `gen1214`, `gen1363`) were reported valid only because their U interval missed upstream's `acos` range.
  - 9 become valid with no report change (`gen0271`, `gen0343`, `gen0485`, `gen1172`, `gen1280`, `gen1414`, `gen1427`, `gen1455`, `wr552_horn_torus_face_m0`). Neither apex is inside the V interval: each face is one sheet of the torus. For `wr552_horn_torus_face_m0`, V [90, 180] ends 2.5e-4 degrees short of the apex (179.99974 against 179.9999985), beyond the 1e-4 degree tolerance. Upstream failed them because their U interval met the `acos` range.
- **Tests:** `wr_539_spindle_torus_reports_an_apex_inside` in `surface/surface/assert_valid.rs`; `crates/solidcore/tests/issue_1672_torus_minus_z.rs`.
