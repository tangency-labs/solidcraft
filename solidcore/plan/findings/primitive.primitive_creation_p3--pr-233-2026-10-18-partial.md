# primitive.primitive_creation_p3 (PR #233, 2026-10-18; partial)

All eight owned functions from `SmPrimitiveCreation.cpp` are ported, including
control flow after dependency calls. Geometry lives in owned curve/surface values;
`PrimitiveAccess` extends the existing `SweepAccess` resolver for B-rep edits,
classification and stitch services. The unit supplies no concrete production
construction adapter. Its default adapters stop only the unavailable calls.
The active `USE_NEW_ANALYTICS` branches are retained: repeated linear sweeps with
one repetition call p1, rotations at least 359.99999 degrees dispatch to full
rotation before the capped-repetition check. The temporary doing-boolean state
is restored on errors; upstream editing-state side effects are preserved.

The final native gate is 287 cases / 280 pass / 0 fail / 6 deferred / 1 rejected,
maximum relative error zero, with no absolute floors. Of the passes, 216 compare
190 values including a constructed ruled surface, 55 compare 110 values including
helper mutations before an error, and 9 compare six status/state/output-count
values on early or empty entry-point paths. Thus these are not 280 successful
primitive constructions. All six nontrivial constructor fixtures execute C++ and
defer in Rust at `topology.brep_p8`; they prove no topology or downstream parity.
The fixture README maps the measured branches to their observed outputs.

The numerical comparison calls the actual file-static offset helper by including
the pinned upstream source with renamed external symbols. It covers ordinary
ruled surfaces, corners/pivots and rational weight matching, circular arcs before
and beyond the apex, surface-map attachment, missing maps, prior marriage/missing
flags and sliver rejection, with threshold-neighbour and 1e-6..1e6 generated cases.
The returned offset map is explicit for the caller's surface attribute store.
Native and Rust curve degrees, control counts, rationality, domains, point and
tangent samples are compared after the call, including failed calls.

Curve-sweep scaling/orientation, face-based skinning, axis classification,
cap extraction/construction, full-topology transformations, stitching and map-copy
tails have full bodies but no compared successful native fixture past the current
construction dependencies. Scaling uses `number_of_unique_knots`, including the
upstream natural-domain bounds and scaled-zero comparison; it must not use the
full knot list length. Subdivision outputs reset on every invocation, including
failed invocations, as `SubdivideAtDiscontinuity` does. Scaling sampling is bounded
to 1,000,000 points; the reused sweep engine bounds repetitions to 4096. There is
no recursion in this unit.

UB-78 was reconfirmed with a direct linked native probe: no skin faces and no
profile curves produces SIGSEGV. Both the shim and checked FFI reject that exact
case before the native call. The fixture is a rejection, never a parity pass.
The helper's selected past-apex three-point-circle construction is preflighted on
an offset copy for existing UB-57, and a non-finite apex placement is rejected.
The port returns checked errors for those undefined paths. No tolerance changes.

Legacy sweep API p3 markers now name the earlier `SmBrep::SmBrep` blocker and point
to the implemented methods; they still need the constructor and concrete access
adapter before calling them. The integrated sweep defaults now resolve B-rep
infinite regions/faces/wire edges and region shells/shell face uses using native
ordered accessors. This exposed a pre-existing fixture mismatch: native op 15
creates an extra shell vertex outside its queried pool, whereas Rust used pool
vertex 0. The fixture now uses the corresponding extra vertex; map count and
pool-key membership are compared separately. Other inherited, unrelated adapters
in dependency PRs are not claimed as implemented by this unit.

Final PR #233 validation: formatting, workspace Clippy, native-feature Clippy,
1,377 kernel tests, wasm32 and the native FFI guard regression pass. All five
affected differential units pass with explicit deferrals: p3 280/6/1 rejected,
p1 667/17/5 rejected, topology sweep 553/14, B-rep sweep API 306/267 and surface
sweep API 831/51 (pass/deferred counts). Perturbation detects 280/280 comparable
p3 cases. The 300-mutation campaign executes 78 cases, rejects 164 and classifies
58 as malformed, with zero differences, crashes, panics or timeouts. This is
bounded harness coverage, not certification of the deferred construction paths.

The full `--green --no-report` gate completes with 210 units: 113 passed, 97 partial, 0 failed.


### PR #233 review fixes (2026-10-19)

Canonical main `28cd7d9a` is merged. Its versions of the seven conflicted kernel
files and B-rep report are retained; the five include!d runners from the review
match main. `CreateSwungPrimitive` keeps the original `Curve` for the profile
start point, path property analysis and path solution point. Only the inheritance
check and `create_swung_surface` use B-spline copies. The curve-sweep preparation
still deliberately slices its copies, as upstream does.

Two exact Rust regressions use imported analytic lines whose cached NURBs differ
by two ULPs: one tests the profile point with no intersection, the other the path
point after a plane intersection. Each checks the analytic result and confirms
that slicing changes it. These regressions verify dispatch; they do not extend
native certification past the unavailable topology construction.

Both `SmStitch::DoStitching` adapters now report `heal.stitch_p2`, matching the
callgraph. The blend call uses the caller's mutable B-rep through the existing
resolver. The FFI guard allows finite negative op-0 tolerances and zero/negative
op-5 approximation tolerance. `review.json` adds four native helper comparisons
and two construction deferrals, rather than treating accepted inputs as coverage
of the topology tail. New p3 result: 293 cases / 284 pass / 0 fail / 8 deferred /
1 UB-78 rejection, maximum relative error zero, no tolerance/floor changes.
Plain/native Clippy, formatting, 1,399 kernel tests, wasm32 and the native FFI
guard regression pass. The other four affected units retain their previous
passing/deferred counts.

Review sensitivity: 284/284 perturbations detected. The 300-mutation campaign
executes 78 cases, rejects 163 and classifies 59 as malformed, with zero
differences, crashes, panics or timeouts. This is bounded harness coverage.

### PR #213 maintainer review fixes (2026-10-19)

Merged canonical main `28cd7d9a` and retained main's complete `topology.brep_p1`
implementation, wiring and reports. The remaining PR diff adds primitive p1 and
its sweep constructor markers/map payload; it does not carry the older p1 co-port.

Both primitive `MakeFaceWithCurves` adapters now call `topology::brep::p7`:
`trimmed_face` supplies one four-curve loop with SAME orientations, and
`detailed_face` forwards each loop, 3D curve, UV curve, orientation and loop point.
Both pass the given region/domain, SAME surface orientation and the upstream
null optional curve-on-surface flag. The cylindrical-box witness now reaches
`SmBrep::MakeShellVertex` in `topology.brep_p9`, rather than stopping at p7.
The pole-free sphere and downstream boolean call sites still cannot complete
the earlier missing dependencies and remain uncertified.

`Boolean2D` now uses `EFF_ZERO_DEG`, the default of upstream `IsParallelTo`
(`SmVector3d.h:143`), instead of the 0.5-degree zone angle. The 0.01-degree
regression returns actual invalid-input status on both engines. The new 32
boolean fixtures compare non-planar first/later surfaces, later primary and
secondary faces, plane offsets and normal tilts around the actual thresholds,
all five operations, empty operands and preserved seeded result slots. Ten
valid coplanar cases execute the C++ and reach Rust's intersection dependency;
they are deferred, not geometry/mutation comparisons. Rust's non-planar fixture
retains the canonical sphere's analytic revolution base (the available Surface
variant); native uses SmSphere. Rust's validation fixture builds surface/face
records while C++ constructs trimmed faces: only validation status, restored
edit flags and result presence are compared, not those fixture topologies.

Two kernel tests pin the angular rejection (including 0.01 degrees), restore
edit/context/result state on error, and observe both plane domains after
coverage and before `IntersectInsertRelate`. These tests prevent a deferred
result from masking regression of the validation prefix. The angular test
accepts 0, 0.00005 and 180 degrees up to the intersection dependency, while
0.0002 and 0.01 degrees must be invalid input.

Rear X tilt is read at op4 index 19 in Rust and C++; four new cone cases vary it
independently. The nonempty sweep layout appends its checked count and curve
blocks; 24 cases cover line, full circle, open arc and skew line with/without
caps and distance 0, 2 or -1. The two negative full-circle cases hang in native
`N_CreateEllipticalArcAngTypeTol` via `SmCone::AdjustSTEPUVDomain` (confirmed in
GDB; see `plan/upstream-issues.md`). An exact native preflight repeats the
upstream full-circle and axis-parallel branch tests and rejects that negative
cylinder branch, preserving accepted negative line/open-arc fixtures. The native
harness transfers sweep curve ownership once, and accounts for Boolean2D
consuming B on success. Deferred nonempty sweeps/cones do not certify their
constructed surfaces or topology. Bottom cylindrical-box points are no longer
added to Vec3::ZERO, preserving negative-zero components.

Current differential gate: **749 cases, 689 pass, 0 fail, 53 deferred, 7 rejected,
max_rel_err 0**, no absolute floors. Deferrals: boolean.topology_intersector_p2
10, topology.brep_p3 5, topology.brep_p8 37, topology.brep_p9 1. The original five
preflight rejections remain, plus the two negative-circle hangs. Every pass
compares real helper values or returned status/state records; no empty passes.

### PR #213 follow-up integration with main 5040b0b2 (2026-10-19)

Merged #130 (`topology.brep_p10`) after the review fixes. Kept main's B-rep p7/p10
code and reports, then reran both affected units: p7 316 cases / 197 pass /
119 deferred; p10 1404 / 839 / 565; both zero failures and zero relative error.
Primitive p1 stays 749 cases / 689 pass / 53 deferred / 7 rejected, zero failures
and zero relative error. Sensitivity still detects 689/689 compared cases.

Validation on this integration: fmt (including both include!d unit files),
workspace Clippy with default and native features, 1397 kernel tests, wasm32,
and five native primitive FFI tests. Two focused 500-case fuzz runs finish with
zero differences, crashes, panics or timeouts: boolean 433 rejected / 31 malformed /
36 ran; nonempty sweeps 413 rejected / 46 malformed / 41 ran. Those executed
fuzz cases include dependency deferrals and are not additional geometry parity
claims. The upstream checkout is clean at the pinned c9c5979.
