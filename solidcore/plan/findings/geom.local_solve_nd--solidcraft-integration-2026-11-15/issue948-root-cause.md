# Issue 948: the N-D correction exposes WR-383 range collapse

Baseline: `be6ecb06420d501a8362d6de3d0de7ef324dafb9`, Rust 1.99.1.
All production worktrees, indices, branches and public state remain untouched by this investigation.
Private logging copies live under `validation/nov15/nd948-trace-root`.

## Finding

**The old range-end refinement accepts an endpoint jumping to its range's start.**
The corrected N-D solver finds the tight root that makes this old guard fail semantically.
This is the existing confirmed WR-383 construction, not a new solver-convergence defect or a demonstrated #16608 split defect.

The outer and inner operand debug snapshots are byte-identical with the old N-D flag off and on.
Their volumes remain `358147.9798812461` and `433334.8951727807`.
The default result has volume `74838.01953194464` with 683 vertices, 1237 edges and 556 faces.
Turning on only the old N-D option returns volume zero with 682 vertices, 1236 edges and 556 faces.
The first topology difference is already present at the public imprint checkpoint.

## The exact corrupting event

The curve/curve global solver has a `RangeOfValues`:

- Start parameters `(0, 0.7632131633423296)`.
- End parameters `(0.7372758233088273, 0.7617633973585559)`.
- End deviation `1.5721731499031345e-5`, under the global `2e-5` request.

The tight `1e-10` refinement, with corrected N-D state, finds
`(2.336339553063178e-9, 0.7632131633377573)`, deviation `3.552713678800501e-14`.
That is a real curve root near the **other** end of the range.
Its point is about `0.7372758 mm` from the endpoint being refined.
The move is not a tiny adjustment within the old endpoint's vertex ball.

`refine_cc_solutions` has `at_end=true`.
Its legacy normalized fractions are `3.1688812778071267e-9` and `3.15377213233603e-9`.
Both pass the unchanged `0.002` shift limit, because the legacy guard measures from the range **start**.
It accepts the jump and collapses the range.

The existing WR-383 correction measures an end from one after orienting the fractions.
Its fractions become `0.9999999968311187` and `0.9999999968462279`.
Both fail the same unchanged shift limit, so the correct guard preserves the range.
Every number above comes from private instrumented execution of the main implementation.
The exact trace lines are retained in the accompanying JSON and in `issue948-range-trace-{off,on}.log`.

A second distant tight root is considered for `at_end=false`.
Its fractions are near one, and both policies already reject it.
That second root must not be special-cased or snapped either.

Call path: `curve::curve::p1::refine_cc_solutions` → `curve::curve::p8::refine` →
`CciGlobalSolver` / `CurveClassification::intersect_edges_mut` → `curve_on_classify_mut` →
`face_intersect_breps_cached` → `TopologyIntersector::intersect_topology`.

## Controls with unchanged original assertions

| Existing N-D option | Existing WR-383 option | Original issue 948 tests |
|---|---|---|
| off | off | 2 pass |
| on | off | 2 fail, successful result has volume zero |
| on | on | 2 pass |

No original input, assertion, expected volume or tolerance is changed.
The main library is the same existing optimized rlib for all controls.
Only the two pre-existing public options differ.

The previously cited WR-614 hold is stale on this baseline.
The entire current `wr_614_coincident_strip_crossing.rs` file runs unchanged in all four combinations:

| N-D option | WR-383 option | Result |
|---|---|---|
| off | off | 12 pass, 0 fail, 0 ignored |
| off | on | 12 pass, 0 fail, 0 ignored |
| on | off | 12 pass, 0 fail, 0 ignored |
| on | on | 12 pass, 0 fail, 0 ignored |

This includes `wr_614_wider_protruding_box_keeps_exit_crossing`.
Its current test requires a valid union of shape `(11,27,18,1,true)` and volume about 256,
then a valid **empty intersection**.
The later crossing-decided-before-intersecting WR-614 work landed after C1's hold.
The separate workflow audit identifies the relevant merged PR and claim boundaries.

## Correct response and limits

The cause is already fixed behind `Kernel::fixes.refine_cc_range_ends` in main.
Pairing that existing correction with unconditional failed-evaluation handling addresses the dependency directly.
There is no evidence supporting restoration of stale Newton values, a tolerance change, a new fallback,
a case-specific branch, or snapping away the tight roots.
Existing analytic endpoint tests already prove both parameter directions and rejection of end-to-start jumps.
The parent must still review scope, remove the obsolete switch without competing with active claims,
and rerun the full affected gates before publishing.

The render/record conversion audit separately finds limitations in the older complete body.
A positive volume and the original passing tests do not alone prove complete body validity.
This investigation proves the range-collapse cause and the controlled recovery of the original tests.
It does not certify every face or pcurve of that larger legacy boolean.

## Archived reproduction evidence

The original controls run `solidcore/crates/solidcore/tests/issue_948_ruled_skin_difference.rs`
and `solidcore/crates/solidcore/tests/wr_614_coincident_strip_crossing.rs` unchanged on the
recorded baseline. Exact range parameters, trace lines, source/build hashes and all control
matrices are archived in [issue948-root-cause.json](issue948-root-cause.json).
Private instrumentation programs, full operand Debug snapshots and build output are not
included in this evidence folder. The event diagram is in
[wr383-schematic/README.md](wr383-schematic/README.md).

All Cargo/rustc and diagnostic executions ran under `nice -n 10`; builds used sccache and two jobs.
No C++ source or forbidden/copyleft implementation was read, built or executed.
