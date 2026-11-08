# #2356 addendum: frozen station plans and uniform correction bounds

Unit: blend.fillet_section (fork-only procedural construction).
Current scope: independently reviewable planning/kernel/enclosure infrastructure,
not a production correction. #2274 and #2322 are both merged; #2390 landed
the overlapping procedural exact-class grading correction. Main
`ba69ec95db369587eaa8315b023eab11f81fd48b` is merged on this branch, including the landed #2369 dependency.

The merge retains main's exact kernel field declarations/ordering and
certificate implementation/comment (including the varying-row lower-bound
fix). The grader uses main's Procedural kind and 1e-9 contact acceptance,
plus this branch's net/bound checks, procedural-off profile and negative
kind/volume controls. No tolerance changes. The inherited arena-notify field
ordering in kernel_adapter is aligned with main, with identical initializers.
There are no dependency-library differences from main: library additions are
only the two new smooth-interior modules and their declarations.

The remaining proof obligations below block production evaluator/net wiring,
not review of this scoped infrastructure: those paths never call these APIs,
and the conditional enclosure explicitly requires independently proved inputs.
Numerical rail checks are build-time measurements, not algebraic zero proofs.
No production acceptance, G2 whole-face certificate or case-gallery change is
claimed. Once the refreshed gates pass, this PR can be ready for review while
#2356 remains open for the independently available production follow-ups.

## Implemented scope

`smooth_interior_plan::SmoothInteriorPlan::prepare` takes the complete
initial station partition explicitly. It stores each interior station's
original parameter, an inward-rounded half-neighbour spacing, and measured
full sided rail jumps through second s/t order. Later certificate refinement
cannot shrink these widths. The plan borrows the definition, preventing it
from surviving a mutation or being attached to another definition; there is
no persistence or transformed owned correction state yet.

Preparation requires both existing procedural/sided opt-ins, finite sorted
stations inside the leader domain, and a G2 definition. It rejects singular
rail jets, discontinuity of the original section's rail t-jets, and failure
of Delta=O(t), Delta.N=O(t^2), Delta2.N=O(t) at either rail. The numerical
checks use 512 machine epsilons times the measured jet scale. These are
build-time checks, **not** proof that a coefficient vanishes exactly.
Tangential jumps allowed by the construction are retained, not suppressed.
`jump_at` uses the full sided definition, including the equal-area weight.

The Q=2 API refuses G3 with a third-order face-smoothness proof requirement.
This is a conservative capability refusal, not the complete production
G3/C2 diagnostic that identifies compatible faces and says what builds.
No existing build changes its result or refusal.

## Uniform conditional enclosure

For each component let r=t(1-t), eps=4 lambda r, Delta=r A, Delta2=B.
The caller must independently prove interval bounds on A and B over the
requested t interval. No sampling API manufactures those bounds here.
The factored kernels of #2384 imply, on either side of a station,

```text
|G| <= 3 eps/16, |G_s| <= 1/2, |G_ss| <= 3/(4 eps)
|G2| <= 3 eps^2/32, |G2_s| <= 11 eps/32, |G2_ss| <= 7/8
|(Delta G + Delta2 G2)_ss| <= 3 |A|/(16 lambda) + 7 |B|/8 = M.
```

The last line cancels r before division. It encloses the rail limit without
an inverse t or an inverse eps. `CorrectionStation::enclose` evaluates these
bounds with outward-rounded intervals; exact rail boxes and boxes outside
the maximum window have exactly zero correction. It also encloses value
and first s derivative. The bounds do not include t/mixed derivatives.

For a span of length h wholly on one side of a station, let L be linear
interpolation of f's endpoints. A Lipschitz derivative with constant M gives
`|f-L| <= M h^2/8`, and each endpoint slope differs from L's slope by at
most M h/2. The two cubic Hermite slope basis magnitudes sum to u(1-u), at
most 1/4. Thus `|Herm[f]-L| <= M h^2/8`, and

```text
|Herm[f]-f| <= M h^2/4.
```

This holds across a moving window edge because the kernel is C2 there.
A fourth-derivative formula cannot automatically be used on a span crossing
that edge: the third derivative can jump there. Spans crossing the station
itself are refused; endpoint slopes use the span's one-sided jet. The bound
is conservative O(h^2), not a claimed sixth-derivative/quintic certificate.
It changes neither the approximation target nor an acceptance tolerance.
This bound is for the Euclidean correction interpolant. It is not directly
a bound for a homogeneous rational net: that integration must derive the
corresponding numerator/weight residual and denominator lower bound.

## What is not certified or enabled

The new enclosure is conditional arithmetic for one correction term, not a
whole-face certificate. Production evaluation/net creation remain unchanged.
The next integration requirements are concrete:

1. Connect complete crossing discovery and the original pre-refinement
   partition to the plan in `GnSectionFsg::build`; supplying some knots is
   not evidence that all crossings were found.
2. Construct the complete rational section jump rows from the two sided
   station series, including weight derivatives. Prove exact rail factors
   and enclose Delta/r and Delta2 over t; numerical zero checks cannot
   justify division by r. Do not erase a residual because it is small.
3. Prove the normal rail factors and nondegeneracy/contact compatibility;
   include t/mixed derivatives for corrected nested-definition box bounds.
4. Combine the original analytic-piece certificate with all correction
   Hermite residuals, and update evaluator/net/station values and derivatives
   together under the project's temporary in-progress flag policy. Corrected station curves are rational in t
   with changed numerators/denominators: build a compatible section basis,
   rather than patching values into the old uncorrected control rows.
   Existing bounds must not be reused for a
   changed definition. Preserve transform/copy/map/serialization semantics.
5. Complete the G3 support-smoothness diagnostic, changed-refusal/volume
   inventory and twisted-wedge/capi/rig/ladder acceptance matrix. Then the
   quintic C2 net and sixth-derivative certificate.

Neither a local station cancellation test nor a conditional interval result
proves whole-face G2. #2330's arc-half seam is unchanged.

## Verification

Executable source tested: `ece7422cf197ddfc5a22dc17584537a08ba8c005`.
Subsequent Rust-source edits only add Markdown backticks to doc comments.

- WSL Ubuntu; CARGO_BUILD_JOBS=2; every cargo/C++ command under nice -n 10.
- Optimized targeted tests: fillet_section 36/36, issue_2356_smooth_interior
  8/8, procedural_blend 8/8, WorkCounters 1/1, and
  wr698_procedural_blend_definition 8/8: all 61 passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed (default features).
- `cargo check -p solidcore --target wasm32-unknown-unknown`: passed.
- `cargo fmt --all --check`: passed.
- Native comparisons: five units passed, 3,658 accepted value-bearing cases, 21 existing rejections, zero failures/deferrals and zero reported relative error. Dev profile, --features cpp --jobs 2; clippy-driver as RUSTC_WORKSPACE_WRAPPER. Shared pinned C++ libraries reused; only FFI shims rebuilt.

No full suite or full --green gate was run. No unit status, mismatch marker,
existing tolerance, production evaluator or production net was changed.
GitHub run https://github.com/tangency-labs/solidcore/actions/runs/41441348154
at 15e1702 completed with action_required and no jobs: no tests ran there.
A maintainer must approve the fork workflow; no source workaround is used.

```text
PASS blend.fillet_solver_p4 cases=651 passed=651 failed=0 deferred=0 max_rel_err=0e0 with_values=651 no_values=0 vacuous=0
PASS blend.fillet_standard_solver_p2 cases=525 passed=525 failed=0 deferred=0 max_rel_err=0e0 with_values=525 no_values=0 vacuous=0
PASS nurbs.b_spline_surface_n_lib_p1 cases=702 passed=702 failed=0 deferred=0 max_rel_err=0e0 with_values=702 no_values=0 vacuous=0
PASS blend.fillet_corner_p3 cases=1013 passed=994 failed=0 deferred=0 max_rel_err=0e0 with_values=994 no_values=0 vacuous=0 rejected=19
PASS blend.fillet_executive_p3 cases=788 passed=786 failed=0 deferred=0 max_rel_err=0e0 with_values=786 no_values=0 vacuous=0 rejected=2
difftest: 5 unit(s), 5 passed, 0 partial, 0 failed
```

## Latest-main integration regression

At merge 657272fa209f91f5ec04a828cd6bf720366cece4, run in Ubuntu WSL:

```sh
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=bin/checks nice -n 10 cargo test --profile difftest -p solidcore --test fork_099cf96_exact_class_cases
```

Result: 4 passed, 1 failed. The failing test is
`with_solidcraft_s_fixes_the_cases_have_the_fork_s_marks_after_the_exact_classes`.
Its `tee_is_fitted` case expects `OverTolerance(0.00147)` but receives
`Built { problems: ["no round face"], off: 0.0 }`. This test enables all
UpstreamFixes except geometric_g2_continuity, including the dependency's
round-on-round flag. The flag-alone, default, WR77 pipe and grading tests
pass. The production source is identical to the previously tested stack;
this newly imported main test exposed an integration failure. The grader
correction below resolves it without changing production geometry.

Postmerge verification at 657272fa2: all 61 targeted tests listed above
passed again; workspace/all-target clippy with -D warnings, fmt --check,
and wasm32-unknown-unknown check also passed. At that head the new main test was
4/5 as recorded above; the later grader fix passes all seven tests. Native difftests were not repeated after this merge:
all library, harness and FFI source are identical to the five-unit tested
75cf76102 tree. No combined-stack green or full-suite claim is made.

## Resolved exact-class grader integration

Investigation after the user's follow-up isolated a test-only cause: the
surface-kind match ignored ProceduralBlend. The round was present, but
never counted or evaluated. A diagnostic on the unchanged production code
measured the tee definition's ball-contact residual as
1.0658141036401503e-14, its stored net's residual as
1.8594022375140185e-6, and its existing certificate bound as
3.987804832360607e-6. Turning only fillet_round_as_definition off gives
a fitted surface with residual 2.6886067583831164e-5, already below the
original 1e-3 tolerance. With fillet_exact_round_classes alone the old
1.4744643375657063e-3 residual remains, as it does by default. Thus the
historical over-tolerance mark is valid for the historical flag profile,
not for all subsequent section fixes enabled together.

The grader now explicitly requires ProceduralBlend for the current
all-fixes fitted case. It samples the definition at the unchanged 1e-9
ball-contact threshold, requires a finite nonnegative stored certificate
bound no greater than the original fitted tolerance, and samples the net
against that original fitted tolerance too. It retains the volume and
validity gates. Default and exact-class-only historical marks remain.
A separate all-fixes/procedural-off control requires a fitted surface
within the original tolerance; no flag-off defect is hidden. Negative
controls reject a definition presented as fitted, a fitted surface
presented as a definition, a wrong volume and wrong analytic surface kinds.

All seven exact-class integration tests pass. These ball-contact grids
are measurements, not an independent proof of the stored certificate or
whole-face continuity. No production code, fix flag or tolerance changed;
this does not enable or certify the #2356 smooth-interior correction.
Latest main fb8fa65e6 was merged first, adding only a findings record.
Follow-up validation in Ubuntu WSL (CARGO_BUILD_JOBS=2, nice -n 10):
seven exact-class integration tests pass; workspace/all-target clippy with
-D warnings and fmt --check pass. Fresh native comparisons pass for
blend.fillet_solver_p4 (651), blend.fillet_standard_solver_p2 (525) and
nurbs.b_spline_surface_n_lib_p1 (702): 1,878 accepted value-bearing cases,
zero failures/deferrals and max relative error 0. Shared C++ libraries
reused; no reference rebuild. Full suite/full --green not run. The prior
wasm check remains applicable to unchanged library code.
## Refresh after dependencies landed

Executable source validated: 5c8c0cd7babc9c7f8a5a46b16ffe729ab0b7c4ac,
merging authoritative main ebe6722e0. WSL Ubuntu, CARGO_BUILD_JOBS=2,
all cargo/C++ commands under nice -n 10.

- 36 section unit tests passed.
- 7 exact-class grader integration tests passed (combined #2390 controls).
- 2 round-on-round dependency regressions passed, with and without WR-714.
- 8 issue_2356 continuity/rail/planning regressions passed.
- 8 WR-698 definition integration tests and 8 procedural-blend unit tests passed.
- Total: 69 targeted tests passed, zero failures.
- Workspace/all-target clippy with -D warnings, fmt --all --check, and
  wasm32-unknown-unknown check passed.

The missing rational-row/normal factors, regularity and mixed-derivative
bounds, corrected rational section basis and combined net certificate remain
production follow-ups. They do not block review of these standalone,
explicitly conditional APIs; no production caller invokes them. No full
suite/full --green, production correction or whole-face G2 claim is made.
Fresh native comparisons on the same executable source also passed:

```text
PASS blend.fillet_solver_p4 cases=651 passed=651 failed=0 deferred=0 max_rel_err=0e0 with_values=651 no_values=0 vacuous=0
PASS blend.fillet_standard_solver_p2 cases=525 passed=525 failed=0 deferred=0 max_rel_err=0e0 with_values=525 no_values=0 vacuous=0
PASS nurbs.b_spline_surface_n_lib_p1 cases=702 passed=702 failed=0 deferred=0 max_rel_err=0e0 with_values=702 no_values=0 vacuous=0
difftest: 3 unit(s), 3 passed, 0 partial, 0 failed
```

1,878 accepted value-bearing cases, zero failures/deferred, maximum relative
error 0. Dev profile, --features cpp --jobs 2, clippy-driver workspace wrapper;
shared pinned reference libraries reused without rebuilding them. These
compare selected default paths, not a corrected smooth-interior C++ surface.
The scoped infrastructure is ready for review; CI and maintainer review
remain required. No production proof obligation has been waived.
## Reference and policy alignment after #2392

The redesigned tangency-labs/smlib construction is the behavioral reference.
The selected local native checks use the historical C++ reference configured
by scripts/upstream-pin.env. They check those default paths only; they do not
establish parity with a corrected fork construction. No new unit promotion,
parked mismatch, changed tolerance or whole-face certificate is justified.

The new APIs add no fix flag, global state, concurrency, one-case branch or
fallback construction. Existing dependency opt-ins are temporary port controls;
when those constructions reach completed parity, their owners promote the
construction and delete its old path and flag. This PR does not make that
lifecycle permanent or claim that the correction is already at parity.

The deterministic crease regressions preceded the frozen-plan/enclosure
implementation. Initial polynomial primitives and their tests shared the first
regression commit; that history has not been rewritten. The production crease
remains a known-defect reproduction, and the conditional APIs never supply a
production surface. No SolidCraft gallery case changes state, so no gallery
row/image change is claimed.
## Verification after the final main merge

Executable source: 7571e71bd6b403625858c5e88439164a873c38cd, merging
main ba69ec95db369587eaa8315b023eab11f81fd48b. Subsequent changes only
refresh these findings. Ubuntu WSL, CARGO_BUILD_JOBS=2, nice -n 10.

Formatting, workspace/all-target clippy with -D warnings and the wasm32 check
passed. All 36 fillet-section unit tests passed, including the conditional
bounds and numerical rail-refusal controls. The earlier stopped verification
attempt did not supply evidence for this merge; these are fresh results.
The remaining focused tests also passed: exact-class grader 7, round-on-round
2, issue_2356_smooth_interior 8, WR-698 definition integration 8, and
procedural-blend unit tests 8. Together with the 36 section tests, this is
69 passing targeted tests, zero failures. No full-suite/full --green or
whole-face-continuity claim is made.

Fresh native comparisons on that same executable source passed:

```text
PASS blend.fillet_solver_p4 cases=651 passed=651 failed=0 deferred=0 max_rel_err=0e0 with_values=651 no_values=0 vacuous=0
PASS blend.fillet_standard_solver_p2 cases=525 passed=525 failed=0 deferred=0 max_rel_err=0e0 with_values=525 no_values=0 vacuous=0
PASS nurbs.b_spline_surface_n_lib_p1 cases=702 passed=702 failed=0 deferred=0 max_rel_err=0e0 with_values=702 no_values=0 vacuous=0
PASS blend.fillet_corner_p3 cases=1013 passed=994 failed=0 deferred=0 max_rel_err=0e0 with_values=994 no_values=0 vacuous=0 rejected=19
PASS blend.fillet_executive_p3 cases=788 passed=786 failed=0 deferred=0 max_rel_err=0e0 with_values=786 no_values=0 vacuous=0 rejected=2
difftest: 5 unit(s), 5 passed, 0 partial, 0 failed
```

3,679 cases, including 21 existing guard rejections; 3,658 accepted
value-bearing comparisons, zero mismatches/deferrals and maximum relative
error 0. Dev profile, --features cpp --jobs 2, clippy-driver workspace
wrapper, shared pinned libraries reused without rebuilding the reference.
These selected historical paths do not establish fork-construction parity.

The standalone infrastructure is ready for review. The production proof
requirements above remain follow-ups, and CI/review remain merge gates.
