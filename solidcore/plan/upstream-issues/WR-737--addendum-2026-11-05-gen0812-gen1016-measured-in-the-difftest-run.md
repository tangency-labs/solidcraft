Updates: WR-737

#### WR-737 (update, 2026-11-05). gen0812 and gen1016 measured in the difftest run itself; confirmed on the C++ reference
- **Severity:** Wrong result
- **Area:** Surfaces and volumes

- **Why:** the 2026-11-04 addendum (`WR-737--addendum-2026-11-04-five-sweep-cases-measured-against-the-bound.md`) confirmed three of the five `surface.surface_assert_valid` sweep cases and left `gen0812` and `gen1016` "not measured that far": an isolated harness saw no report from either side, where the difftest gives `values.out.len` cpp 149, rust 3. This is that measurement, taken inside the difftest run.
- **Confirmed:** yes, for `gen0812` and `gen1016` of `surface.surface_assert_valid`, on the difftest's own C++ reference. The C++ emits exactly one report, `SmBSplineSurface` test #5 (`15002`, test 5, `Bad NonMonotonic CPts`, owner `SmSphere`), on a collapsed pole row whose steps are one ulp of the coordinate and inside the rounding bound; solidcore emits no report. Nothing else differs.
- **How:** nf-v5 on main `0beef3b82`, `cargo run --profile difftest -p difftest --features cpp -- --unit surface.surface_assert_valid --filter <case>`, with a temporary `eprintln` (not committed) in the runner printing the case's block and both sides' decoded `out` vectors, and one in `assert_valid` test #5 printing the net, upstream's absolute verdict (`grid_has_non_monotonic_points`) beside solidcore's (`grid_has_non_monotonic_points_to_rounding`), and every nonzero step shorter than 1e-6 with its bound `(2 u + gamma_3) (|P0| + |P1|)`.
- **What each side emits:** C++ `[ok 0, reports 1, <test #5 report>, last_add 0]` (149 numbers); solidcore `[ok 1, reports 0, last_add 0]` (3 numbers), for both cases.
- **The net solidcore reads** (both spheres with `make = 1`, rational nets of 5 x 9 and 9 x 5 points): upstream's absolute criterion reports it non-monotonic on that net (`abs = true`), as the C++ does, and the rounding bound passes it (`rounding = false`). The only nonzero short steps are the collapsed pole row's eight, each one ulp of the coordinate:

| case | sphere | collapsed row | steps | step | bound | distance from origin | bound / step |
|---|---|---|---|---|---|---|---|
| gen0812 | r 147309, centre (1.21e5, -3.78e4, -1.99e4), level 0 | row u = 4, v = 0..8 | 8 | 1.455e-11 | 1.982e-10 | 1.785e5 | 13.6 |
| gen1016 | r 1127997, centre (-7.76e5, 9.08e5, 3.30e5), level 2 | column v = 0, u = 0..8 | 8 | 1.164e-10 | 2.092e-9 | 1.885e6 | 18.0 |

  So both are this entry's defect, the same reading as `gen0528`, `ovf0160` and `ovf0276`: one ulp above an absolute `SM_EFF_ZERO` read as a direction that reverses at every point.
- **Why the isolated harness differed:** not C++ process state. `ffi::eval` on the bit-exact block the difftest generates, called from a fresh `cargo test` process with no other case run first, returns the same 149 numbers for both cases. The report depends on the input's last bits: the same block rounded to 12 significant digits gives no report for `gen1016` (3 numbers), and rounded to 10 gives none for either, while 15 and 17 digits keep it. Whether a pole row's points land one ulp apart is decided by the exact centre, axes and radius, so a harness fed a block that is not bit-identical to the generated one sees no report on either side. The earlier harness's code is not on record, so which rounding it applied is not known; an input that differs in its last bits is the cause these measurements leave.
