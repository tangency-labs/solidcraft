Updates: WR-505

#### WR-505 (update, 2026-11-14). `internal_pole_row_through_knot` is removed

`SmBSplineSurface::HasInternalPole` always tests the row through each interior knot (`knots - 1 - degree`, tangency-labs/smlib#41), not one row further for each repeated copy of the knot. The old row is gone, with the field, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** 6 rows of `surface.surface_assert_valid` move.
  - `gen0094` and `gen0699` are horn tori (R = r) whose V interval [-180, 180] has the pole on the axis only at its ends. They lose B-spline test 2 ("Bad Internal Pole") and become valid. Upstream tested the corner row beside the boundary pole at the circle's double knots.
  - `gen0025`, `gen1228` and `gen1456` are horn tori whose V interval runs through V = 180 (or -180) inside it, where the tube passes through the axis: a real internal pole. They gain test 2. Upstream's row one step further missed it.
  - `ovf0263`, a degree-1 (V) B-spline whose V knot 0.25 is repeated 3 times, more than degree + 1, completes with its surface discontinuity and knot multiplicity (test 1) failures, plus test 5, which runs only once no pole is found. MEASURED: the net is the generator's mode 9, so it closes in U (its last U row repeats its first, closing all five V columns), and test 5's 170-degree turn-back check trips only on column 4, at 172.9 degrees between P2→P3 and P3→P4. Column 0's v = 0 boundary also crosses itself, near u ≈ 0.515 and 0.82 (sampled). Before, it was an error, because upstream's row index left the net. At such a knot there is no row to compare (the coincidence loop would run zero times), so both walks now skip it instead of reporting a pole untested. The test proves only that skip: `a_knot_repeated_past_degree_plus_one_is_not_a_pole`.
- **Tests:** `issue_1515_horn_torus_has_no_internal_pole` and `issue_1515_real_pole_at_a_double_knot_is_found` in `surface/surface/assert_valid.rs`; `crates/solidcore/tests/issue_1511_revolve_disc_touching_axis.rs` (the quarter horn torus passes the whole-body check).
