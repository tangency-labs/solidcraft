# general: Failed topology evaluations and incomplete healing stages (#521, PR #700) (2026-10-24)

Migrated PR #700 record. Later addenda describe the final scope; older draft
claims and validation figures below retain their original bases. Its upstream
issue IDs WR-257 through WR-263 are now WR-260 through WR-264, WR-266 and
WR-268. The Stage3-classification report moved from WR-265 to WR-268 after main
merged a different WR-265; the underlying evidence is unchanged.

Related addenda: [scope after #704](general--521-topology-evaluation-status--addendum-2026-10-24-after-704.md), [cache-wrapper integration](general--521-topology-evaluation-status--addendum-2026-10-25-cache-wrapper.md), [cache-owner validation](general--521-topology-evaluation-status--addendum-2026-10-25-cache-owner.md).

A Hermite wire edge is a small defined failure: `SmHermiteCurve` inherits the base
`ReverseParameterization`, which returns `SM_ERR`. `SmEdge::ReverseOrientation`
drops that status, installs the unset output interval and flips the edge uses.
The Rust port followed it. The fixed Rust call propagates the curve error before
changing the edge interval, orientations or UV trims. The curve method's own
mutation behavior remains its implementation's responsibility.

The other implemented paths named by #521 now require successful evaluations:

- `vertex::are_edges_tangent` and `is_topological_vertex` propagate tangent and
  `AngleBetween` errors. A zero/too-small tangent no longer supplies a zero-angle
  certificate. Healthy parallel edges, cusps and the angle thresholds keep their
  previous behavior.
- `loopuse::uv_corner_boundaries` evaluates fresh endpoints for each edge use and
  propagates trim/3D evaluation and fallback point-drop errors. An unsuccessful
  drop returns `SM_ERR`. Trim acquisition already propagated its error.
- `edgeuse::p1::start_point_uv_loop_gap` propagates both curve evaluations and both
  drops, including unsuccessful drops; `calc_dist_and_dev` propagates failed first
  derivatives. A distance-only request still uses the stored solution value.
- `FaceProps::set_props_stage3` restores the owned classification tree and problem
  type, then propagates classification errors before reading its output arrays.
  An error restores the stage marker to Stage2 while keeping partial diagnostics.
  A subsequent call retries instead of returning success from a stale Stage3A.

`query::loop::contains_uv_point` already propagates evaluation failures and uses
Unknown safely for failed trim acquisition. `HasDogLegNearPole` has no Rust
implementation to audit yet. The change covers the implemented paths above.

### Reproduction and reference evidence

Seven regressions in the existing test modules failed before these changes:
failed Hermite reversal, original tangent/angle error propagation, failed trim
corner evaluation (at the first and a later edge use), UV/3D gap evaluation,
failed surface derivatives, failed loop classification, and an incomplete healing
stage retried after a dangling loopuse. The first five failed together; the two
healing regressions were separately reproduced before their production change.

A standalone probe against the unchanged pinned C++ libraries on macOS arm64
constructed the Hermite from points `(0,0,0)`, `(1,0,0)` and directions `(1,0,0)`,
`(0,1,0)`, then called `CreateWireEdgeFromCurve` over `[0,1]` and reversed the edge:

```text
Hermite reverse status=1001 output=[1e+20,-1e+20]
wire status=1000
edge before=[0,1] after=[1e+20,-1e+20] orientation=1 -> 2
tangent length=0 angle status=1007 value=0
tangent length=1e-12 angle status=1007 value=0
tangent length=1 angle status=1000 value=0
```

Twenty new cases in the existing `topology.vertex/constructed.json` cover constant
B-splines at either end of a chain, both orientations and endpoint selectors, plus
healthy line controls. The preserved pre-fix binary matched C++ in all 20; the fixed
kernel matches 14 and propagates the failed angle in six. Additional existing cases
exercise point curves and `1e-12` tangents. Raw C++/Rust outputs were inspected for
all 43 angle differences and four healing differences, plus the 14 new controls.
The temporary diagnostic example was removed.

| Angle case group | Cases | C++ TRUE | C++ FALSE | Rust result |
|---|---:|---:|---:|---|
| New constant-tangent cases | 6 | 6 | 0 | `status_1007` |
| Existing `point_curve_*` cases | 7 | 7 | 0 | `status_1007` |
| Existing `rev_tiny_scale*` cases | 30 | 16 | 14 | `status_1007` |

Those 43 cases concern lost failure statuses. Fourteen C++ predicates already
return FALSE; their difference is the missing error status. The difftest runner
preserves this specific angle error as `status_1007` instead of translating it to
the null-dereference sentinel or a harness error. Every case still runs both sides.
The 43 WR-261 records keep `topology.vertex` partial.

Four existing healing cases (`stage3`, `gen0345`, `gen0821`, `gen0957`) retain status
1040 on both sides. Their only compared differences are the incomplete stage marker:
C++ 5 (Stage3A), Rust 4 (Stage2), including a subsequent diagnostic snapshot in
`gen0345`. The four WR-266 records keep these diagnostic differences explicit.
The unsafe empty-NURB native operations used by some Rust-only regressions were
not executed against C++; their failed-output paths were checked in source.

### Validation on `6ecdaaa21467a52461f1d39bc3e4c0d9a745988b` plus this change

Fmt, default/native workspace Clippy with warnings denied, wasm, 2086 kernel unit
tests, five integration tests and 48 runner tests pass. The existing unsupported
partial-sphere/torus integration case remains ignored. The native matrix covered
13 units; vertex and face_props were then rerun after recording their observed
C++ differences and marking vertex partial. The other eleven passing units' code,
fixtures and per-unit known records did not change.

| Unit | Cases | Matching | Known | Deferred | Rejected |
|---|---:|---:|---:|---:|---:|
| topology.edge | 1787 | 1787 | 0 | 0 | 0 |
| topology.vertex | 5528 | 5485 | 43 | 0 | 0 |
| topology.loopuse | 8854 | 8854 | 0 | 0 | 0 |
| topology.edgeuse_p1 | 4402 | 4402 | 0 | 0 | 0 |
| topology.edgeuse_p3 | 4789 | 4789 | 0 | 0 | 0 |
| topology.face_p1 | 2315 | 2276 | 2 | 37 | 0 |
| query.loop | 3040 | 3040 | 0 | 0 | 0 |
| query.face_p3 | 1199 | 1087 | 0 | 112 | 0 |
| query.face_p4 | 500 | 395 | 0 | 105 | 0 |
| boolean.merge_p1 | 248 | 243 | 0 | 5 | 0 |
| topology.brep_p1 | 2627 | 2461 | 0 | 150 | 16 |
| topology.brep_p5 | 797 | 795 | 0 | 2 | 0 |
| heal.face_props | 1528 | 1427 | 4 | 97 | 0 |

Total: 37,614 cases, 37,041 matching (including 646 both-error comparisons),
49 known differences, 508 deferrals (471 UNPORTED plus 37 native UB-137 crash guards),
16 native input-guard rejections and
zero unexpected failures; five units green and eight partial. The 49 consist of
47 differences explained above plus two existing main Rust/harness MoveSeam
limitations in `topology.face_p1`. Those two are not C++ defect claims.
All 17 pre-existing main mismatch records remain. No comparator tolerance, floor,
reference implementation or FFI rejection guard was changed. The full macOS
`--green` reader/hash/deformation baseline (#448-450) was not claimed.
