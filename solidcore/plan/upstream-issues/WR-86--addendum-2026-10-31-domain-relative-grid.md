Updates: WR-86

#### WR-86 (update, 2026-10-31). Domain-relative rectangular grid, opt-in

- **What changed:** The port now has an opt-in fix for the rectangular branch of
  `SmFace::GetPointsInFace`. With `Kernel::fixes.face_points_span_domain` on, each
  column starts at the UV domain's minimum u plus the column increment, each row starts
  at the domain's minimum v plus the row increment, and the column increment is the
  domain's u length over the number of columns plus one. The default remains the pinned
  C++ grid (fractions of the domain's lengths from zero), so difftest parity is
  unchanged.
- **Location (current pin):** `source/SMLib/src/SmFace.cpp:21455`-`21677`,
  `SmFace::GetPointsInFace` (def. `:21455`); the wrong grid is `:21493`-`:21515`. The
  original record's definition at `:21167` and location `:21206`-`:21230` are from an
  older upstream pin.
- **Effect:** on a rectangularly trimmed face with UV domain `[3, 4] x [-7, -6.5]`,
  `GetPointsInFace(1, ..)` returns UV `(1/3, 0.25)` by default and `(3.5, -6.75)` with
  the field on. On `[0, 1] x [0, 2]` count 1, the default is `(1/3, 1)` and the field
  gives `(0.5, 1)`. For count 7 over a unit u length, the column positions are
  `1/15`..`7/15` by default and `1/8`..`7/8` with the field on. The V increment and the
  per-point V dither are computed as before.
- **Coverage limit:** this corrects the origin and the column spacing, not uniform
  coverage of the domain for every count. Counts above 30 still plan only
  `min(count, 30)` columns and stop once the requested count is reached, so when those
  rows do not divide the requested count the last sampled column is truncated and later
  columns are not sampled (31 points fill 16 of 30 columns). The planner and iteration
  order are deliberately unchanged.
- **Randomness:** both modes draw the same number of V dithers in the same order from
  `kernel.rng`; only the grid they are added to changes. The tests compare the next
  draw after a call with the field on and off from equal seeds.
- **Native evidence:** with more than one point the C++ reference seeds its V dither
  from the clock, so the existing `points_dom_offset_1`-style runner does not compare
  the full random UV list. This addendum claims no flag-on C++ parity and no full
  native geometry comparison for counts above 1. The count-1 real UV, XYZ position and
  normal default parity is already covered by the existing fixture; the new tests
  exercise the opt-in geometry and the flag behaviour in Rust.
- **Field:** `Kernel::fixes.face_points_span_domain` (`crates/solidcore/src/kernel.rs`),
  off by default per AGENTS.md section 4b.
- **Tests:** `topology::face::p2::tests::points_in_face_span_domain_samples_from_the_domain`
  and `topology::face::p2::tests::points_in_face_span_domain_boundary_counts`. The
  existing `points_in_a_rectangular_face` default-pin test is unchanged.
