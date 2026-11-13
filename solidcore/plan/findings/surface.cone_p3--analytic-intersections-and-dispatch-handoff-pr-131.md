# surface.cone_p3: analytic intersections and dispatch handoff (PR #131)

The cone part-3 port keeps complete analytic and NURBS intersection curve records,
UV curve presence, classifications and deviations in its differential protocol.
The direct methods operate on bare geometry; face-owned tolerance context is not
available through the standalone `Cone` type.

Two unfinished callees are isolated in `surface/cone/p3.rs`:

- `ellipse_trim_intervals`: wire to `surface.cone_p2`'s
  `sm_GetEllipseTrimIntervals` when PR #71 lands. This enables oblique elliptical
  plane sections and the trimmed ellipses of intersecting equal-radius cylinders.
  PR #71 currently takes `&Ellipse`, while this stub retains raw center/axes/radii;
  whichever PR lands second must adapt the signature and rerun cone part 3.
- `boundary_intersections`: #120 merged `Surface::intersect_surface_boundaries`,
  but its operands require a `SurfaceKind::Cone` adapter absent from official main.
  This adapter belongs to active #76/#133; the stub now names `surface.surface_p2`.
  Do not substitute a B-spline/revolution wrapper: it erases Cone/Cylinder virtual
  methods. Once the typed adapter merges, wire the boundary API and rerun every
  formerly deferred case without claiming its output is certified in advance.
  The owned caller already retains upstream's second-call status overwrite, duplicate
  removal order, coincident classification and UV-array ordering.

The global dispatch entry is `Cone::global_surface_intersect_with`. Its general
intersector callback receives the original cone, both domains, the other surface,
both edge flags and both optional tolerances. This preserves cone virtual methods
until the shared enum can hold cones with the caller's Kernel (issue #76); it does
not substitute a default Kernel or a plain B-spline/revolution. Plane, cone,
revolution and extrusion dispatch are present. Sphere dispatch and integration in
`Surface::global_surface_intersect` await their concrete enum kinds. In particular,
upstream's `SmCylinder_TYPE` other operand intentionally does not match its
`SmCone_TYPE` switch arm.

Resolved regression `cylinder_ends_near_touch`: two radius-1, height-3 cylinders
with origins `(0,0,0)` and `(1,0,3.00000005)`, full sweeps and approximation tolerance
`1e-7`. C++ returns success with no curves; the first Rust translation returned
`SM_ERR` (1001). `SmSurfOfRevolution::STEPInversion` explicitly writes EXTERIOR
when its generator solve returns no solutions, before returning SM_ERR. The native
intersection ignores that status and skips the EXTERIOR result. The port now
recognizes that specific no-solution error and skips it without fabricating or
reading an unwritten UV coordinate. Other errors, including Unported, propagate.

### Apple Clang/libc++ baseline limitation observed during PR #131

The complete `difftest --green` run on the Mac aborts with SIGABRT (exit 134)
after `nlib.math`, inside the existing `topology.database_io` unit. This is
independently reproducible without running cone code:

```sh
nice -n 10 bin/difftest/difftest --unit topology.database_io --filter gen0185 --no-report
```

Generated seed: `573100365250327377`. Running the 59 database fixtures separately
also shows libc++ numeric-extraction differences: `surface_sms_ascii`'s first
compared double is C++ `-7777.25` (the extraction sentinel) versus Rust
`4.404800473510645`; overflow/underflow, incomplete exponents and hex extraction
fixtures disagree too. The Rust reader explicitly implements libstdc++ semantics.
The source of `SmDatabaseIO::ReadType` contains unbounded extraction into
`char sBuff[SM_MAXSIZE]`; this is a candidate for the generated abort, but an abort
backtrace was not obtained. No reader semantics, fixtures or tolerances were
changed to hide these platform differences. Full certification of the project
suite still requires the supported GNU C++/libstdc++ CI environment.

Local validation used separate Clang compatibility patches; those patches are
excluded from this unit PR after scope review and preserved at `d8d20ff1` on
`support/apple-clang-harness`. The two affected harness units passed independently:
`geom.geom_utility_p1` 4185/4185 and `curve.projected_curve` 2727/2727, both
bit-identical. `surface.cone_p3` passes 1187 cases with 121 dependency deferrals;
a 10108-case stress run has 9048 passes, 1060 dependency deferrals and zero
failures, all compared values bit-identical. Perturbation detects every one of
1187 comparable default cases.

Crossing-axis coverage uses equal radii and an axis intersection inside both finite
segments, including generated tiny/huge scales and subdomains; these cases reach
`sm_AddEllipseIntervalsTrimmedToTwoCones` and explicitly defer on cone part 2.


### PR #131 review follow-up against earlier ports

Review examples: [#130 resource-exhaustion note](https://github.com/bherbruck/solidcore/pull/130#issuecomment-6206051059),
[#125 native-guard and status review](https://github.com/bherbruck/solidcore/pull/125#pullrequestreview-5540211429),
and [#123 branch/order/perturbation review](https://github.com/bherbruck/solidcore/pull/123#issuecomment-6205894305).

Found and fixed an overbroad native guard: `GlobalSurfaceIntersect` switches on
exact `SmCone_TYPE`; a `SmCylinder_TYPE` target goes to the general intersector.
The missing-optional-tolerance guard now follows that dispatch before checking the
coincident analytic-cylinder path. A native regression uses two coincident
radius-1, height-3 surfaces with the second constructed as `SmCylinder` and no
optional tolerance; the old guard returned `SC_ERR_ARG`, while the corrected
adapter completes the actual native general call. The Rust callback dispatch has
an existing matching regression. This native safety test is not counted as a
Rust/C++ numerical comparison.

The added native matrix covers direct/global plane and cone calls, both first
operand classes, swap/inside-out flags, full/subdomains and omitted, zero,
positive, negative and system-default tolerances (320 calls). Fourteen constructor
boundary probes exercise zero/negative radii and heights, tiny height, zero,
reversed and oversized sweeps, zero axes and scaled axes. Malformed/nonfinite
record checks remain FFI contract tests rather than artificial difftest passes.
All `solidcore-ffi` tests were run locally with `cpp`, serially; the default
workspace tests do not enable this feature. The proposed CI step was removed
after scope review: this port PR leaves repository workflows unchanged.

The resource audit of `surface/cone/p3.rs` found no link-following walks, recursion,
file readers, input-declared allocation sizes, or numeric convergence loops. Its
loops traverse fixed pairs, returned curve/interval collections, or the existing
output prefix during deduplication. Periodic interval intersection in the ellipse
helper is currently behind the `surface.cone_p2` deferral and must be rechecked
when that dependency is wired. This audit does not certify unported dependencies.

Coverage limits remain explicit: oblique ellipse trimming, intersecting-axis
ellipse output and coincident-boundary deduplication are ported but cannot yet be
compared beyond their dependency entry points. Revolution/extrusion global
branches have Rust implementations but this unit's native runner constructs only
plane/cone/cylinder targets; sphere/shared-enum dispatch remains a handoff gap.
The review did not promote these paths to green or count them as compared.

Review validation: 1308 default cases, 1187 bit-identical comparisons, 121
explicit dependency deferrals, zero failures; all 1187 perturbations detected.
The 10108-case stress run has 9048 comparisons, 1060 deferrals and zero failures.
All 32 new tolerance fixtures compare actual returned geometry/classifications
rather than synthetic errors (`cases_both_errored = 0` for the complete report).
All 33 native FFI tests, formatting, and strict workspace Clippy with the C++
feature pass after the review changes and merge of official main `04a2b106`.

### PR #131 contribution scope

The repository contribution guide requires one PR per port unit. Scope review
removed the proposed CI workflow step and the three shared Apple Clang/runtime
fixes from this PR; existing native safety tests remain in the unit's FFI module.
The platform patches are preserved in commit `d8d20ff1` on the local
`support/apple-clang-harness` branch for separate submission. This PR has no CI
workflow or unrelated-unit shim/build-script delta against official main.

The recorded Mac native runs used those platform patches as build prerequisites.
They do not certify a clean upstream harness build on macOS. The supported
Linux/GNU full gate still requires maintainer approval for the fork workflow.

Independent read-only subagent review found no confirmed numerical or safety
defect in the cone implementation. Its 384 extra horizontal-cut cases at
radii/heights `1e-12..1e12`, with default/zero/negative/large tolerances, produced
294 full curve comparisons and 90 matching constructor-status results, zero
mismatches and zero deferrals. Those exploratory checks used existing compiled
artifacts; they do not replace the committed report or certify the scoped head's
clean native build. The review confirmed the documented deferred/uncovered paths
and recommended the CI/platform scope separation above. Formatting and strict
plain workspace Clippy pass after that separation.

### PR #131 maintainer review follow-up

[Review 5543522828](https://github.com/bherbruck/solidcore/pull/131#pullrequestreview-5543522828)
reported 1308 cone cases (1187 bit-identical passes, 121 dependency deferrals),
181 units with zero failures in the maintainer's full GNU run, and 212 extra
adversarial cases (174 exact comparisons, 31 deferrals, seven guarded refusals,
zero mismatches). These are the reviewer's results for the prior numerical
revision, not certification of later edits or the latest CI run.

1. Workflow and build-script changes were already removed at `20189dd0` and
   remain absent. The maintainer explicitly reserves those changes for a separate
   maintainer PR.
2. The optional-tolerance null dereference is now UB-123 under Crash / undefined
   behaviour, with standard How found / Confirmed fields. It remains source-found
   and unconfirmed by execution of undefined behaviour.
3. The SSI corpus builder's missing cone/cylinder variants now name
   `surface.surface_p2`, matching the adjacent torus dispatch gap. A source check confirms no
   `UNPORTED(surface.cone_p3)` markers remain in `crates/`.
4. Coincident-boundary deduplication checks both result bundles' UV, type and
   deviation vector lengths against their 3D curves before removing anything.
   The regression covers each vector being shorter/longer in either bundle
   (16 malformed combinations), returning `SolidError::sm_err(OP)`.
5. The general callback's result replaces the analytic result, matching
   `SmSurfaceIntersector::DoIntersection` resetting all outputs. No currently
   compared path returns analytic curves and also requests general intersection.
6. The module Deviations now state that start-plane transform failures propagate
   in Rust, while native STEPInversion initializes EXTERIOR before that transform
   and its cone caller skips the failed point. This is the reviewer's permitted
   documentation option, not a newly claimed parity fix.

After merging official main `7962b462`, all 1192 kernel tests pass, including the
new malformed-boundary regression. Formatting, strict plain workspace Clippy
and the wasm32 check also pass. Native revalidation uses the separate disposable
checkout and platform prerequisites described above; they are not submitted.

Fresh native merged-tree comparison passes: cone_p3 1308 / 1187 / 0 / 121,
surface_p2 869 / 858 / 0 / 11, and trimming_tools 3263 / 2842 / 0 / 421
(total / pass / fail / defer). All three reports reproduce byte for byte.
Sensitivity detects 1187/1187, and the 10108-case cone stress run has 9048 exact
passes, 1060 dependency deferrals and zero failures. The focused three cone tests
pass after adding a well-formed, unequal-curve-count bundle pair to the boundary
contract regression. The simple SSI ownership retag is checked directly rather
than adding a test that only repeats its static labels.
Strict C++-feature workspace Clippy also passes in the disposable validation
checkout. The platform prerequisites remain excluded from the submitted branch.

### surface.cone_p3 fuzz follow-up (reversed angles, axis overflow)

- `fuzz00891`: Rust `angle_in_sweep` snapped and compared against the normalised sweep
  (`SmExtent1d` swaps a reversed range) while `SmCone.cpp:3385-3390` (and four more sites)
  use the raw `GetStartAngleDeg`/`GetEndAngleDeg`. A cone with start 0 / end -1 degree gave
  a line in Rust and none in C++. Fixed to use the raw angles; the periodic mapping keeps
  the sweep. Fixture `fuzz_reversed_angle_range`.
- `fuzz01093`: a frame axis with a component of about 1e154 or more makes `Unitize`
  overflow to a zero vector (upstream-issues WR-159; faithful in Rust). With such a Y
  axis the cone's polar circle cannot convert some angles on either side: the point
  inversion in `ConvertToNURBSParameter` finds no answer (`SmEllipse.cpp:587-596`,
  status 1001 in both). The disagreement was the caller: `SmCone::IntersectWithCone`
  ignores the `ConvertUVFromSTEPToNURBS` status (`SmCone.cpp:2966-2967`) and tests the
  partly written `SmPoint2d` against the domains (2970-2972), while the Rust used `?` and
  failed the whole intersection. A default `SmPoint2d` is `SM_UNDEF_DOUBLE`, so the output
  is defined: U is the polar table's guess (0.9167 and 0.8333 in the reduced case), V is
  `SM_UNDEF_DOUBLE`. Rust now mirrors that (`nurbs_uv_status_ignored` in
  `surface/cone/p3.rs`); no FFI guard (the input is not UB in C++). Fixtures
  `fuzz_axis_overflow_zero_y` (two lines), `fuzz_axis_overflow_zero_y_trimmed` (the
  written U falls outside a trimmed domain for one line) and `_swapped` (swap UV: the
  failed conversion returns before the exchange).


### surface.cone_p3 tiny-axis follow-up (issue #207)

- A tiny Y axis can leave a cone with a NURB but no polar circle: `MakeNurbWithSweepParams`
  ignores the converter rebuild's failure (`SmSurfOfRevolution.cpp:1033`). In the parallel
  cylinder branch, `IntersectWithCone` reaches `ConvertUVFromSTEPToNURBS` at
  `SmCone.cpp:2966-2967`, which enters `SmPolarConversion::ConvertToNURBSParameter` and
  dereferences the missing circle at `SmEllipse.cpp:491` (upstream-issues UB-179).
- The native shim overrides that virtual conversion for real `SmCone` and `SmCylinder`
  subclasses and rejects only when the conversion is actually called with a null circle.
  This preserves self-intersection/disjoint early returns, constructor errors, and
  converters that have a circle even when their exact conversion table is unavailable.
  A dedicated exception exits the native call with `SC_ERR_ARG`; owned native objects
  unwind normally. Neither a blanket small-axis check nor `HavePolarConversion()` is used.
- `GetHeight` also encountered the missing circle, but its native behavior is defined:
  `SmPoint3d` members initialize to `SM_UNDEF_DOUBLE` (`SmVector3d.h:56-58`). Both ignored
  `EvaluatePoint` failures leave those points unchanged, so their difference, and the
  resulting height, are zero (`SmCone.cpp:2264-2268`). Rust previously propagated status
  1006. `Cone::height` now preserves the default point on ignored evaluation failures;
  `Unported` still propagates. The part-1 state adapters now compare height without the
  former polar-circle restriction, and `height_without_polar.json` checks tiny axes and
  negative radii with every swap/inside-out combination.
- `tiny_axis.json` has eight `rejected_*` guard assertions (both operand orders, direct
  and global cone calls, real cylinders, zero and sub-threshold axes) and eight defined
  neighboring calls (self/disjoint early returns and nondegenerate axes). Guard assertions
  are status-only tests of `SC_ERR_ARG`, not geometry parity; Rust intersection is not
  executed for them. Four `missing_polar_height_plane_*` cases exercise the corrected
  height through direct/global plane intersections; two compare a returned circle.
  The original overflow-axis fixtures remain covered.
- Before the height fix, a 10,000-case probe of the original 1,312-case corpus found
  eight status-1006 disagreements, all retained in `fuzz_height.json`: fuzz00226,
  fuzz02052, fuzz02407, fuzz05279, fuzz05558, fuzz06289, fuzz06674 and fuzz09442.
  It also found three status-1007 disagreements on global calls (fuzz04485, fuzz04639,
  fuzz07170). Those reached the runner's general-intersector callback, whose temporary
  `InvalidInput` hid the existing missing typed Cone adapter. The callback now returns
  `Unported(surface.surface_p2)` for that adapter, as the cone's boundary path already
  does. `fuzz_general.json` retains those cases as dependency deferrals; no general
  intersection result is fabricated or counted as a pass.
- Affected-unit comparison with the polar set-up bound passes on Linux:
  `PASS surface.cone_p1 cases=20702 passed=20702 failed=0 deferred=0 max_rel_err=0e0`;
  `PARTIAL surface.cone_p3 cases=1415 passed=1289 failed=0 deferred=126 max_rel_err=0e0`.
  Part 3 has 1281 passes with values (72 of them the `angle_bounds.json` cones) and
  eight native-guard status assertions (Rust runs on them; its result is not
  compared). All eight
  status-1006 regressions and all four reduced plane intersections pass. Its 126
  deferrals comprise 91 on `surface.cone_p2` and 35 on the typed Cone adapter in
  `surface.surface_p2`; the latter includes the three general fuzz cases and two
  valid-axis global neighbors whose native general fallback cannot yet be compared.
- A 10,000-case fuzz of the final corpus found one timeout, fuzz02755: a global
  cone/cone call whose second cone has start and end angles near `9.2e18` degrees. The
  Rust side spun in `Ellipse::set_up_polar_conversion`'s whole-turn shift loop (upstream
  `SmEllipse.cpp:278-289`), which never ends once adding 360 no longer changes the
  midpoint; a well-formed native record with the same angles also does not return
  (upstream-issues HANG-23). The fuzzed record itself had 18 doubles, so the native
  wrapper refused it, but the harness still runs Rust on wrapper refusals. The Rust loop
  now stops with `NotConverged` after `MAX_PERIOD_SHIFTS` (2^20) turns.
- The `surface.cone_p3` FFI first accepted angles up to `360 * (2^20 - 3)` degrees, the
  largest the bounded set-up reaches. Fuzzing seeded from cones at that bound then found
  three native timeouts (fuzz05643, fuzz07048, fuzz07266: the bound plus a tolerance of
  `-1` or `0`, or a radius of 8). gdb showed each in `SmExtent1d::PeriodicWrap` under
  `SmEllipse::STEPInversion` / `SmPolarConversion::ConvertToPolarParameter` /
  `SmCone::ConvertUVFromNURBSToSTEP`, reached from `SmSurface::DropPoint`,
  `SmSurfPtSolveStepper` and `SmSurfaceIntersector::TraceCurve`: every evaluation wraps
  the angle one turn per iteration (HANG-1, ported faithfully), so the cost grows with
  the angle on both sides. The FFI therefore accepts start and end angles only within
  upstream's documented `[-360, 360]` (`SmCone.h:205-206`), as `surface.cone_p1` does for
  circles. No earlier fixture uses angles outside it; `angle_bounds.json` (72 cases)
  compares cones at both limits, a full negative turn and a straddling domain with every
  swap and inside-out flag, through direct and global cone and plane intersections.
  After the change: `FUZZ-PASS surface.cone_p3 cases=10000 rejected=4242 malformed=3032
  ran=2726 differs=0 crashes=0 panics=0 timeouts=0`; perturbation detects 1289/1289
  (`cone_p3`) and 20702/20702 (`cone_p1`); `curve.ellipse_p1` (2028) and
  `curve.ellipse_p2` (2102) still pass bit-identically; after merging official main
  `f1d0f1d5`, `--green` reports 203 units, 113 passed, 90 partial, 0 failed.
- After merging official main `61670af3` (which wires `surface.cone_p2`, PR #153), on
  Linux: `PARTIAL surface.cone_p3 cases=1415 passed=1380 failed=0 deferred=35
  max_rel_err=0e0` (1372 with values plus the eight native-guard assertions); all 35
  deferrals are the typed Cone adapter in `surface.surface_p2`. `surface.cone_p1`
  (20702), `surface.cone_p2` (1550) and `curve.ellipse_p2` (2102) pass, reports
  unchanged.
- Linux validation uses Proxmox VM 107 (`runner-codex-02`, x86-64, GNU C++ 15.2), a fresh
  snapshot of official main `53ba102b` plus this change, and the shared C++ reference build.
  Its allowed source trees were byte-checked against pinned `c9c5979`. The first default
  runner build was killed by signal 9 near the 8 GiB VM memory limit. Validation retries
  use `--config 'profile.difftest.package.difftest.opt-level=0'` for the runner package
  only; Rust kernel and C++ reference settings are unchanged. No Apple Clang prerequisites
  are used.
