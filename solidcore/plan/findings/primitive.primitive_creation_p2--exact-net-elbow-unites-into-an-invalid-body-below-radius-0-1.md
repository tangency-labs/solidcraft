# primitive.primitive_creation_p2: the exact net lets a small elbow's union succeed with an invalid body (2026-11-05)

Cases `gen0092` (seed 11582637188108233846) and `gen0189` (seed 11582637188108233943), the same
input: op 9, `CreateSweepAlongPlanarPath` of a circle of radius 1e-3 (centre at the origin,
normal +Z) along two lines (0,0,0)-(0,0,1e-2) and (0,0,1e-2)-(1e-2,0,1e-2), tolerance 1e-7,
both caps, profile moved to the path. Part of #2794.

Measured on main with #2740 (main's difftest sweep, `difftest-sweep-reports`):

| field | C++ | Rust |
|---|---|---|
| `values.record[0]` (status) | 1001 | 0 |
| `values.record[2]` (`DoingBoolean` left set) | 1 | 0 |
| `values.record[10]` (a B-rep returned) | 0 | 1 |

Before #2740 both cases passed with both sides failing alike: the C++ and the pre-#2740 Rust return 1001 from
the union of the two mitred pieces (`boolean::merge::p3`, `SM_ERR`), leaving the flag set. #2740
builds a path sweep's net from its definition (`PathMotion::create_exact`) instead of fitting it.
Reproduced the old result by making `create_curve_sweep` keep the fitted net (the `.filter` on the
exact net false): `Upstream { operation: "boolean::merge::p3", code: 1001 }`, the C++'s status.

**Solidcore's success is not a correct body.** `check_brep` (level 2) on the result:

| radius | leg | tolerance | failed checks |
|---|---|---|---|
| 1e-1 | 1 | 1e-7, 1e-3 | 0 |
| 1e-1 | 1e-1 | 1e-7 | 0 |
| 1e-2 | 1e-1 | 1e-5, 1e-3 | 14 (edge/edge gaps of 9.4e-3, `SmBrep::Test #61`, `#82`) |
| 1e-3 | 1 | 1e-7 | 2 |
| 1e-3 | 1e-2 | 1e-7 | 2: `SmVertex::Test #1 Bad Vertex Gap`, offset 1.8284271e-3 = r(2 sqrt 2 - 1), tolerance 2e-5 |
| 1e-3 | 1e-2 | 1e-3 | 5 |

The leg does not matter, the radius does: from radius 0.1 the elbow is a valid body of volume
pi r^2 (leg1 + leg2) (test `issue_2794_planar_path_mitred_elbow`); at 1e-2 and below the union
succeeds and leaves vertices off their faces by the order of the radius. So the exact net removes
the failure that made the C++ and the old path refuse, and shows the defect behind it: the mitre
at a profile this small (50 times the kernel's 2e-5 and less). Not traced; the fixed 0.1
reference offsets in the mitre trim (`planar_path.rs`, `before[0] + (-before[1] * 0.1)`, upstream's)
are not the cause alone, since a leg of 1 fails the same.

Solidcore returns an invalid body where the C++ refused.
Both cases are listed in `crates/goldens/expected/known_failing.json` (no golden line) until the small-profile mitre is fixed (the correct answer for
radius 1e-3 is the valid body of volume pi r^2 2 leg = 6.2832e-8, or a refusal).
