# blend.fillet_executive_p3: four cases #2718 moved, measured; one is solidcore's too-small refusal, three were the refusal check writing to the body (2026-11-05)

- **Unit:** `blend.fillet_executive_p3`.
- **Cases:** `preview-radius-tiny`, `source-file-box_w_slot-all-edges`,
  `source-file-cyl_on_box_w_fillet-all-edges`, `sweep4-sphere_diff_3cylinders-all-r0.1`.
  main's sweep showed them as harness errors (the runner did not map the typed
  `SolidError::BlendRefused`); with the runner comparing every error by its
  `SolidError::sm_status` they fail on values.
- **Not the blend corner default.** Main just before #2708 (`f4e14b6cf^1`, the old corner
  code present and `fillet_blend_corner` off, as every `UpstreamFixes` field defaults), with
  the runner's status fix applied, gives all four the same first mismatch and values as
  main after #2708. So none of them is a case of
  `plan/findings/blend.fillet_corner_p3--blend-corner-default-diverges-from-cpp-v1-0-1-2026-11-04.md`.
- **Which change.** All four pass in a #2400 branch report of 2026-11-04 05:44 and on main
  at `c249eb979` (before #2718), and fail at #2718's merge `7309bf19c`. Inside #2718 they
  pass at `c769832e0` and fail at `d27234ac8`, "a round too small to change the part is always
  refused; no flag", which runs `round_too_small` before every constant circular round's
  preparation (it was behind `fillet_refuse_rounds_too_small`, off, until then). Each probe:
  the commit checked out with the runner fix, `cargo run --profile difftest -p difftest
  --features cpp -- --unit blend.fillet_executive_p3 --filter <case>` against the same
  C++ reference (`$HOME/work/to-2400-cpp`).

## `preview-radius-tiny`: solidcore's refusal, by design

A 10 x 10 x 10 box, edge 0, `SmApiFilletPreview` at radius `1e-12`.

| | status | `values.preview.len` |
|---|---|---|
| C++ v1.0.1 | 0, one preview surface | 111 |
| solidcore | 1001 (`SM_ERR`) | 5 (status, 3 counts, 0 surfaces) |

solidcore returns `SolidError::BlendRefused { code: 1001, refusal: RoundTooSmall { edge:
EdgeId(6), turn: 1.5707963267948966, tolerance: 1e-5, smallest_radius:
2.414213562373096e-5 } }` (read with a temporary `eprintln` in the runner's preview error
path, not committed): across faces turning by pi/2 a round of radius r lies
`r (sec(pi/4) - 1) = 0.414 r` inside the edge, and at `1e-12` that is far below the edge's
tolerance `1e-5`, so the round is the edge itself to the model's accuracy and solidcore refuses
it naming the smallest radius that rounds it. Upstream builds a preview of a degenerate
round there. This is solidcore's result (#2718, d27234ac8; AGENTS.md section 4b after
#2647: a fix is the default), so the case is a known mismatch: the reference is not
wrong, and solidcore does not follow it here.

## The other three: the check wrote to the body (fixed in this PR)

| case | field | C++ | solidcore before the fix |
|---|---|---|---|
| `source-file-box_w_slot-all-edges` | `values.result_geometry[11]` (edge tolerance) and 35 more | 1e-5 | 1.4868940702423615e-5 (1.03e-5 to 1.49e-5) |
| `source-file-cyl_on_box_w_fillet-all-edges` | `values.result_geometry[35]` (edge tolerance) and 10 more | 1e-5 | 1.029868189778811e-5 |
| `sweep4-sphere_diff_3cylinders-all-r0.1` | `values.mass_properties[0]` and 89 more | 3.823683329993967e4 | 3.8236232190085895e4 (rel 1.57e-5) |

None of these rounds is too small: the check refuses nothing in them. It changed them through
the body. `round_too_small` reads the edge through `GetBlendEdgeuse` and `EvaluateBinormal`,
which build the UV trim curve an edge use lacks and refit the edge's tolerance to it. On
`cyl_on_box_w_fillet` a `{:#?}` dump of the target before and after the check (temporary,
not committed) differs on edges 8, 10 and 13 to 17: `uv_trim_curve: None` became a fitted
B-spline and edge tolerances went from `1e-5` to `1.0298681907738848e-5`, the value the
result then carries. Upstream's preparation finds the body without them. Passing
`use_3d_only` to the binormals is not enough (the same dump still differs: the blend edge
use query builds them too); running the check on a copy of the body makes all three pass.

The fix (this PR): `FilletSession::refuse_rounds_too_small` runs `round_too_small` on a
copy of the target made once per call, so the check is a query and the body reaches the
preparation as the caller gave it. Test: `a_round_refused_as_too_small_leaves_the_body_as_it_was`
in `crates/solidcore/tests/blend_refusal.rs` (on #2176's plate, ten of 46 edge uses
without a trim curve got one from the check alone; a refused round left them on the body).
