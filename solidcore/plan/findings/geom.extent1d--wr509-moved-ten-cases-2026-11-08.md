# geom.extent1d: ten golden cases WR-509's ContainsPeriodicValue fix moves, measured (2026-11-08)

- **Change:** `Extent1d::contains_periodic_value` moves a value within the tolerance of the top
  of the period above the interval's start down by the period, and keeps it where that lies in
  the interval, instead of setting it to the start (WR-509, tangency-labs/solidcore#1518).
- **How measured:** `cargo run --profile goldens -p goldens -- --all --goldens --jobs 4
  --no-report` on the fix (rebased onto main `9dc37ee02`) and on main `9dc37ee02`, on one
  machine, compared unit by unit. 279 of 283 units give the same result. The four that move
  pass every case on main on that machine, so their other cases agree with the recorded
  values; the ten lines below were blessed with `--bless --filter <case>`, nothing else.
  The machine is Windows (where 74 other units differ from the recorded values on main
  either way, tangency-labs/solidcore#487); Linux CI is the check of record.

## `geom.extent2d` `my_test_extent2d_contains_periodic_tol`: upstream's MyTest, the case WR-509 names

`Extent2d::contains_periodic_point` of u = 8.9, period 4, on `[1, 3]`, tolerance 0.2:
`periodic_point[0]` 1.0 (the start) becomes 0.9000000000000004 (8.9 two periods down, inside
the tolerance). This is the change the WR-509 record predicts.

## `geom.extent1d`: five cases, the distances of a value kept a period down

| Case | Field | Was | Now |
|---|---|---|---|
| `edge_periodic_near_top` | `values[1]`, `[4]` | 0.0 | -9.999999974752427e-6 |
| `edge_periodic_near_top` | `values[2]`, `[5]` (the distance to the start: the fields the `abs` flips) | 0.0 | 9.999999974752427e-6 |
| `edge_periodic_near_top` | `values[3]` | 90.0 | 90.00000999999997 |
| `gen0870` | `values[2]` | 0.0 | 1.000088900582341e-12 |
| `gen0870` | `values[3]` | 1.000088900582341e-12 | 0.0 |
| `gen1503` | `values[2]`, `[5]` | 0.0 | 3.9968028886505635e-15 |
| `gen2063` | `values[2]`, `[5]` | 0.0 | 2.975050761300224e-16 |
| `gen2267` | `values[2]`, `[5]` | 0.0 | 3.131175874138137e-16 |

`edge_periodic_near_top` is the case the fix is for: a value 1e-5 below the start of `[0, 90]`,
given a period up, was set to the start (distance 0 to it, 90 to the end); it now keeps its
place, 1e-5 before the start, and its distance to the start is 1e-5 (a distance: the
kept value's is taken as `abs`, as `assert_valid` reads it with `min2`). The four generated cases are the same move by rounding-sized
amounts: the value's distance from the start is now its own, not 0.

## `curve.ellipse_p2`: three cases, the record's block sums

| Case | `values.rec.block[0]` was | now |
|---|---|---|
| `gen0042` | `[247.59502854734214, 267.6372546180361, 10403.49931860245]` | `[247.59500994892622, 267.637273216452, 10403.498983830963]` |
| `gen0855` | `[-156.02409151847633, 410.5124223109191, -4682.4589894044575]` | `[-156.02408784125566, 410.5124186336983, -4682.458967341133]` |
| `gen1601` | `[245.6209888918087, 260.6012897777472, 8049.2557268355995]` | `[245.62098559210304, 260.60129307745285, 8049.255707037365]` |

These are not points: the golden stores a long array as blocks of `[sum, absolute sum,
weighted sum]` (`crates/goldens/src/golden.rs`), and block 0's three sums move by at most
3.3e-4 (`gen0042`'s weighted sum, 3.2e-8 relative). Which entries of the record moved, and by
how much each, was not traced, and the new values are not checked against an independent
value. The expected route is the ellipse's STEP inversion, which maps its angle through
`contains_periodic_value` (the WR-509 record); that is not confirmed case by case.

## `query.face_p3` `adv_sec_b8_e2_m0_f1e-09_c2_t0.0_0.0`: a case that now reaches the query

`sphere.smb`, face 0, edge use 2, the line's crossing a fraction 1e-9 along the edge from its
start (`fixtures/query.face_p3/review_adversarial.json`). On main the harness rejects the case
before `SectorClassifyCurveOn` (`error: "rejected"`, one of `resolve`'s input checks); with the
fix it reaches the query, which returns `sector` `[1, 1, 5]`: inside 1, possible problem 1,
problem 5, so the classification itself flags a possible problem there. A point 1e-9 past the
start of a periodic interval is the input WR-509 changes; which of `resolve`'s checks rejected
it on main was not traced, and the sector is not checked against an independent
classification: the golden records it, it does not prove it right.
