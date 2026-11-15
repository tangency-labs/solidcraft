# intersect.curve_class_p8 (`SmCurveClassification` natural boundaries and discontinuities; PR #152)

Ports all five owned members from `SmCurveClass.cpp:8462-10420`: natural 3D/UV
boundaries, UV/3D surface discontinuities (including upstream's misspelled
`Discontinuties` symbols), and volume discontinuities. UV/3D forwarding uses
shared nonrecursive helpers. Natural boundaries keep native traversal order,
orientation, `IsDegenerate()`'s `SM_EFF_ZERO` default, left/nonzero-tangent flags,
strict gap comparisons, temporary NOTHING labels, and classification propagation.
The surface owner is cleared on an exact evaluation copy, preserving the caller's
owner and NURBS data. The part-3 interval lookup exposes its initialized result
on ordinary failure so these callers can retain upstream's ignored-status behavior.

After the main merge, the inline `SetInterval` accessor and `pub(super)` curve
resolver are inherited from landed #150. The remaining shared delta is visibility
for the initialized `FindInterval` helper and the p8 module declaration. Landed
dependencies enter through official main; no open dependency branch is bundled
or recertified.

Dependency boundaries and integration handoffs:

- Nonempty boundary/iso-curve solutions stop exactly at `sm_InsertSolutions`
  (`intersect.curve_class_p5`, #150). Wiring it will require registering temporary
  boundary handles with the caller's geometry adapter; the stub retains the
  actual boundary curve/plane, solutions and explicit tolerance for that handoff.
  The numeric helper is now inherited from main; the retained deferral denotes
  the reviewed temporary-boundary handle/geometry registration gap.
- The general volume plane/curve call is `SmSurface::GlobalCurveIntersect`
  (`surface.surface_p8`, #118). Native uses the curve's natural interval, ignores
  its ordinary status, and may still consume solutions. That output lifecycle
  must be retained when replacing the stub. The implementation is now inherited
  from main; plane handle registration and the WR-155 buffer lifecycle remain
  the reviewed integration boundary.
- Real `SmCrvOnSurf` iso-curves now retain the existing concrete surface, including
  offset wrappers, with checked exact snapshots and native temporary-owner semantics.
  They are not replaced by B-spline stand-ins. Cache/break classification can still
  reach the independent construction/context dependencies recorded in its report.
- Incoming p8 sites in `curve.crv_on_surf` and `curve.crv_in_volume` are after
  their earlier `intersect.curve_class_p4` construction stubs (#137). Part 4 is
  still independent. Stale p8 UNPORTED comments now name the available methods;
  the actual errors still correctly name part 4. Those sites need part-4
  construction/context plumbing before they can reach p8; no unit is promoted.

The native oracle builds real segments, full circles and stationary quadratic
B-splines; planes, flat/warped C0 grids with independent U/V counts; identity and
C0 B-spline volumes. Fixtures include rectangle-side geometry from
`TestSmCurves.cpp:424-430`, branch-derived probes, nonunit surface domains,
subintervals, seams/touches, preclassified intervals, default/negative/zero
source tolerances and tiny/huge geometry. The plan-listed tests reach this unit
indirectly; no direct calls to these five members occur in the permitted test
folders. Generated cases use the harness's recorded fixed seeds.

Coverage limits: solution insertion, crossing-driven partition mutation and the
following crossing-angle classifications are not certified. All discontinuous
volume plane paths are deferred; UV-curve rejection and no-discontinuity volume
returns compare status and unchanged state only. Natural-boundary midpoint classifications and UV-discontinuity fallback/
propagation are the directly compared classification subset;
3D surface-discontinuity construction/cache paths remain deferred throughout.
partition parameters remain unchanged on these paths and are checked as state,
not reported as newly solved intersection parameters.
preclassified skip/propagation cases are not new topology construction. The
native status is carried with integer labels and interval state, so the report's
`cases_both_errored` field is not a count of non-success native status results.
No absolute floors or tolerance changes are used. The original fixture subset
used finite magnitudes up to `1e12` and UV spans of at least `1e-6`. These were
coverage bounds, not native crash preconditions. Review fixtures now include
`1e13`/`1e14` coordinates, `1e-8` UV spans, tiny and negative circle radii, and
classification intervals outside the natural curve domain. Wider finite inputs
are admitted by the FFI validator; unexercised values remain uncertified.

Source review found UB-155 and WR-154..WR-156 (entries in upstream issues). These
are source findings, not isolated native reproductions. V+V slot selection,
W-from-V coordinates and U-only unaligned checking are retained where defined;
invalid indices/empty arrays return errors instead of undefined reads. The volume
borrowed-buffer alias is not reproduced by the independent Rust vectors; this
behavior remains wholly behind the plane-intersection deferral and must be
reviewed before that dependency is wired. Native fixtures use equal V/W volume
lists and never deliberately execute the known out-of-bounds case.

Validation below is for `3eaa99f8`, after merging official main `535f53d0`.
Earlier snapshots and interrupted pre-merge builds are not used to certify this
revision. Native validation uses an isolated Ubuntu 26.04.1 x86_64
checkout, GCC 15.2.0, Rust 1.98.0, and the shared pinned C++ reference. The allowed
SMLib and SM_API source trees match clean local upstream `c9c5979` by recursive
checksum comparison. A validation-only opt-level-0 override applies to the CLI
adapter; the Rust numerical kernel stays at difftest opt-level 1, and native flags
stay `-O2 -ffp-contract=off -fno-fast-math`. No profile/workflow/build-script change
is submitted.

The final default report has 1,076 cases (276 fixed + 800 fixed-seed generated),
766 matching states, no failures, and 310 explicit deferrals. Of the compared
cases, 595 exercise surface-operation states, 119 check defined UV-curve volume
rejection and its state, and 52 check the no-discontinuity volume early return.
All 214 actual 3D surface-discontinuity cases defer; 2D inputs to its entry point
exercise forwarding only. No crossing partition or discontinuous-volume output
is presented as certified.

```text
PARTIAL intersect.curve_class_p8 cases=1076 passed=766 failed=0 deferred=310 max_rel_err=0e0 blocked_by=curve.crv_on_surf(52),intersect.curve_class_p4(162),intersect.curve_class_p5(55),surface.surface_p8(41)
SENSITIVE intersect.curve_class_p8 perturbed=766 detected=766 unperturbed=0 deferred=310
PARTIAL intersect.curve_class_p1 cases=2125 passed=2048 failed=0 deferred=77 max_rel_err=0e0 blocked_by=curve.crv_on_surf(24),curve.iso_curve(22),intersect.curve_class_p4(6),topology.edge(11),topology.face_p2(12),topology.vertex(2)
PARTIAL intersect.curve_class_p3 cases=3292 passed=2064 failed=0 deferred=1228 max_rel_err=0e0 blocked_by=intersect.curve_class_p2(886),intersect.curve_class_p7(232),topology.edge(31),topology.face_p2(38),topology.vertex(41)
PARTIAL curve.crv_on_surf cases=2094 passed=1787 failed=0 deferred=307 max_rel_err=0e0 blocked_by=intersect.curve_class_p4(307)
PARTIAL curve.crv_in_volume cases=2030 passed=1855 failed=0 deferred=175 max_rel_err=0e0 blocked_by=intersect.curve_class_p4(175)
```

The four existing reports reproduce byte for byte and are excluded from this PR.
Formatting, strict plain workspace Clippy, 1,209 kernel tests and WASM pass.
Strict native-enabled workspace Clippy (difftest profile, all targets) also passes.
The three new kernel regressions cover volume early-return ordering, empty
classification safety and owner preservation. Both new harness regressions pass
with the native feature: one FFI malformed-record test and one result-bundle
length test (shorter/longer records and a valid bundle). `--status-of` prints
`partial`. `port-pr-review` was applied to the whole diff, native branches, handle
resolution, output/status lifecycles, constructor guards, recursion/resource use,
and revision-specific evidence; the uncovered paths above remain explicit.

Full native gate after recording p8 as partial:

```text
difftest: 187 unit(s), 99 passed, 88 partial, 0 failed
```

The p8 report from this full run is byte-identical to its standalone default run.

### PR #152 FFI guard review follow-up

The FFI guard now rejects exactly zero circle radii (including negative zero),
retaining valid tiny and negative radii. The finite-magnitude cap, minimum-positive
UV-span cutoff and natural-curve-domain restriction were removed. Positive UV
domains, positive surface scales, finite values, record/interval structure and
the degenerate-line check remain validated. The module header distinguishes
coverage limits from crash guards. `review_guards.json` adds 39 fixed native
regressions across operations 0–2: five nonzero radii, two large-coordinate
magnitudes, independent tiny U/V domains and line/circle/stationary-B-spline
intervals outside their natural domain. Exact-zero radii are rejected without
calling the native constructor.

Follow-up source revision: `939daa17`, on the reviewed official base `535f53d0`.
Formatting, strict plain workspace Clippy, all 1,209 kernel tests and WASM pass.
Strict native-enabled workspace Clippy and both FFI regressions with the native
feature also pass. Native validation uses a fresh Ubuntu 26.04.1 x86_64 checkout,
GCC 15.2.0 / Rust 1.98.0, and a fresh reference build whose 492 allowed SMLib/SM_API
source files match clean pinned `c9c5979` by checksum. The same validation-only
CLI-adapter opt-level-0 override is used; kernel opt-level 1 and native flags
remain unchanged. No submitted profile or harness changes are needed.

The refreshed default report contains 1,115 cases (315 fixed + 800 generated),
785 exact comparisons, zero failures and 330 deferrals. Every original case's
report entry is unchanged; the 39 review cases add 19 comparisons and 20
`intersect.curve_class_p5` deferrals. All 785 perturbations are detected.
The compared subset is now 614 surface-operation states, 119 defined UV-volume
rejections and 52 no-discontinuity volume early returns. The expanded finite
inputs exercise safe native calls; the 20 new insertion paths remain uncertified.
The preceding 187-unit full gate is previous-head evidence and was not repeated
for this isolated wrapper/fixture correction.

```text
PARTIAL intersect.curve_class_p8 cases=1115 passed=785 failed=0 deferred=330 max_rel_err=0e0 blocked_by=curve.crv_on_surf(52),intersect.curve_class_p4(162),intersect.curve_class_p5(75),surface.surface_p8(41)
SENSITIVE intersect.curve_class_p8 perturbed=785 detected=785 unperturbed=0 deferred=330
```

Latest official main was inspected at `b2ebc087`; it now includes parts 5/6/7
and surface part 8 and conflicts with this branch. Its trial merge was backed
out to keep this guard-only correction on the reviewed base. The part-5
accessor/visibility conflict and numerical dependency integration remain a
separate merge handoff. The ignored-status and volume-alias limitations above
remain unchanged; this follow-up does not open those deferred paths.

### PR #152 main conflict resolution

Integrated official main `f72938d4` in source revision `d691bb64`. The module
list retains parts 5/6/7/8, and part 1 now reuses main's single `set_interval`
implementation and `pub(super)` curve resolver. Both branches' findings are
preserved; this p8 section is placed before the later main additions so future
appends can merge independently. No numerical p8 implementation or FFI guard
changes were needed for the conflicts, and the reviewed adapter deferral
boundaries remain unchanged. The prior unmerged-main handoff is superseded.

Formatting, strict plain and native-enabled workspace Clippy, all 1,254 kernel
tests and WASM pass on the integrated source. The fresh p8 default report and
full-suite p8 report both match the reviewed report byte for byte. All 785
perturbations are detected. The complete native gate passes with 196 units:
108 pass, 88 partial, zero failures. No other unit's status is changed.

Validation uses the same Ubuntu 26.04.1 / GCC 15.2.0 / Rust 1.98.0 environment
and pinned C++ reference, with the CLI-only opt-level-0 override described above.
The allowed pinned `TestFiles/pt_TestFiles` corpus was populated before the full
run; the initial attempt lacking those fixture files was stopped and discarded.
The final gate used `nice -n 10 bin/difftest/difftest --green`, the exact CLI built
by the successful `cargo run` for source `d691bb64`; the strict native Clippy run
completed alongside it. The C++ build scripts/patches are unchanged from the
verified reference build, so the existing libraries were reused.

```text
PARTIAL curve.crv_in_volume cases=2030 passed=1855 failed=0 deferred=175 max_rel_err=0e0 blocked_by=intersect.curve_class_p4(175)
PARTIAL curve.crv_on_surf cases=2094 passed=1787 failed=0 deferred=307 max_rel_err=0e0 blocked_by=intersect.curve_class_p4(307)
PARTIAL intersect.curve_class_p1 cases=2125 passed=2048 failed=0 deferred=77 max_rel_err=0e0 blocked_by=curve.crv_on_surf(24),curve.iso_curve(22),intersect.curve_class_p4(6),topology.edge(11),topology.face_p2(12),topology.vertex(2)
PARTIAL intersect.curve_class_p3 cases=3292 passed=2156 failed=0 deferred=1136 max_rel_err=0e0 blocked_by=intersect.curve_class_p2(1022),topology.edge(32),topology.face_p2(41),topology.vertex(41)
PARTIAL intersect.curve_class_p8 cases=1115 passed=785 failed=0 deferred=330 max_rel_err=0e0 blocked_by=curve.crv_on_surf(52),intersect.curve_class_p4(162),intersect.curve_class_p5(75),surface.surface_p8(41)
SENSITIVE intersect.curve_class_p8 perturbed=785 detected=785 unperturbed=0 deferred=330
difftest: 196 unit(s), 108 passed, 88 partial, 0 failed
```

### PR #152 part-5 and surface part-8 wiring (maintainer)

Both p8 back-edge stubs are now wired (AGENTS section 4a):

- `sm_InsertSolutions` -> `p5::insert_solutions` at all three native call sites.
  Each passes the explicit tolerance (`XSectTol3d`, or `XSectTol2d` for UV) and
  propagates status (`SER`), as native does. Native records the temporary boundary
  curve or plane pointer as the end object and frees it on return. The shim numbers
  these dangling pointers in order of first appearance. Rust names temporaries with
  `ClassObject::temporary(index, kind)`, a new defaulted trait method (`None`:
  insertion returns invalid-input), CO-PORTED in `intersect.point_class`. A geometry
  wrapper resolves every temporary of the call, because native keeps all of them
  alive until return. The difftest runner numbers them the same way as the shim.
  Solution objects map curve/analytic-curve to the classification curve and
  boundary/analytic-boundary to the temporary. Collapsed short ranges are copied
  back, so the crossing loops see native's in-place edits.
- Volume planes call `Surface::global_curve_intersect` over the plane's natural UV
  domain and the curve's natural interval, ignoring ordinary status. An ordinary
  failure yields no solutions, where native may keep partial output.
- WR-155: `sSolutions4` and `sSolutions5` borrow `aData4`. `ReSet`/`Add` keep the
  borrow, so slot 5's final (inserted) solutions overwrite slot 4's leading entries
  while slot 4 holds at most 20. The port emulates that. When slot 5 holds more
  than 20 over a borrowed, nonempty slot 4, it returns invalid-input, because the
  buffer then holds intermediate state. Intermediate `SetSize(0)` un-borrowing
  inside the tree solve's combine step is not modelled. Twelve `wr155_*` fixtures
  exercise the alias: vertical, downward and slanted lines through both W boundary
  planes, single-plane controls and a preclassified case, each in parameter space
  and in space. Every crossing insertion currently defers deeper to
  `intersect.curve_class_p2` (`InsertAt`), so the emulation is not yet compared.
  Disabling it changes no current comparison.

```text
PARTIAL intersect.curve_class_p8 cases=1127 passed=853 failed=0 deferred=274 max_rel_err=0e0 blocked_by=curve.crv_on_surf(52),intersect.curve_class_p2(60),intersect.curve_class_p4(162)
```

The p5 (75) and surface_p8 (41) deferrals are gone: 68 of those cases now compare
exactly and 48 defer deeper to p2 (plus the 12 new wr155 fixtures).
