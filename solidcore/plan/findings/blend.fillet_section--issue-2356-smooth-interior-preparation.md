# blend.fillet_section: finite-smoothness interior preparation (2026-11-02)

Issue: <https://github.com/tangency-labs/solidcore/issues/2356>.
PR: <https://github.com/tangency-labs/solidcore/pull/2384>.

## Scope and reproduction

This is preparatory work on the approved option-A construction. Its original
dependencies #2322 (sided definition evaluation) and #2274 (procedural rounds)
are now merged. Current main ba69ec95d is merged into this branch, including
#2390; useful grader controls from both branches are retained. This PR does
not implement a corrected production definition. Its scoped planning and
conditional-enclosure APIs are independently reviewable; the missing production
proofs are explicit follow-ups described in the frozen-plan enclosure finding.
The redesigned fork construction is the behavioral reference. The local harness
still uses its configured v1.0.1 C++ pin for selected historical paths; those
comparisons do not establish fork-construction parity. No numerical tolerance
or unit status is changed. The new measurements are Rust regressions rather
than a comparison with a corrected C++ implementation.

Run from the worktree in Ubuntu WSL, with `CARGO_BUILD_JOBS=2`:

```sh
nice -n 10 cargo test --profile difftest -p solidcore --test issue_2356_smooth_interior -- --nocapture
nice -n 10 cargo test --profile difftest -p solidcore --lib smooth_interior
```

The minimal support is `z = 0.2 (x - 5) + 0.3 y + 0.008 (x - 5)_+^3`,
represented by a cubic B-spline with one simple interior knot. The other
support is `y = 0`. A radius-2 ball crosses the knot at leader parameter
`s = 0.533499186293079`, obtained by projecting the known crossing centre
onto the leader. The leader joins exact ball centres at top-foot x = 1
and x = 9. A control omits the cubic term, retaining the same spline
representation. Inclination in both x and y keeps the contact direction
from hiding the third-partial jump; the uninclined extrusion was checked
and does not reproduce the normal crease.

The tests check that the top is C2 and not C3, measure both sides of the
definition at 33 section heights, and check position, tangent and normal
curvature contact at both rails. A separate test builds the generator's
actual net, checks every interior knot line at 33 heights, and checks that
the net reproduces the definition's sided first s derivatives at the
crossing. The small direct-definition fixtures use a placeholder net only
to construct a definition; that placeholder is never used as net evidence.

Known-crease assertions deliberately require a jump greater than `1e-6`.
They are defect reproductions, not loosened acceptance gates: when the
production correction lands, replace them with the issue's `1e-9` gate.
Analytic control, rail and corrected station-jet checks already use `1e-9`.
The normal angle uses `atan2(|N_left cross N_right|, N_left dot N_right)`;
`acos(dot)` cannot resolve the specified near-zero angle reliably.

The optimized WSL run measures a definition normal jump of
`1.3045711394729534e-4` rad and a section-direction normal-curvature jump of
`1.665909306281184e-3`. The actual built net has 29 spans and 28 interior
knot lines; its maximum normal jump is `1.304571139500838e-4` rad. The
planar control's net has two spans and a maximum measured normal jump of
`5.157236908654451e-16` rad. All five integration regressions pass,
including the corrected station's partials through order two at `1e-9`
and the rail controls. These values belong to this minimal fixture, not
the issue's twisted-wedge capi case.

## Closed-form kernels and a station derivative detail

The Q=2 endpoint Hermite conditions uniquely give

```text
H(u)  = 3/16 + u/2 + 3u^2/8 - u^4/16
H2(u) = 3u/32 + u^2/4 + 3u^3/16 - u^5/32
G     = -eps (1 - |u|)^3 (3 + |u|) / 16,  |u| < 1
G2    = d G / 2
```

`smooth_interior::correction_kernels` evaluates their values and sided first
two d derivatives. Tests independently differentiate H/H2, check the unit
jumps, zero endpoint jets, invalid widths, and shrinking-window scaling.

At `d = 0`, `G2_s = -3 eps/32` on **both** sides. Therefore

```text
S~_s(s_j,t) = (S_s(before) + S_s(after))/2 + 3 eps Delta2/32.
```

Taking only the average, as one sentence of the issue's net staging brief
suggests, omits the second-jump kernel's derivative. This is a consequence
of the approved endpoint conditions, not a different smoothing construction.
It was reported on the issue before integration. The corrected-station test
includes this term and uses jumps of the full equal-area section, including
the thumbweight's derivatives, to cancel all first and second partial jumps.

## What remains before production use

The current certified bound applies to the **uncorrected** definition. It
cannot be attached to a corrected definition or its changed net. Production
wiring must first supply a correction-aware interval error bound. In
particular, the width tends to zero at a rail, so separate bounds on
`Delta` and inverse powers of `eps` lose the cancellation needed there.
The closed-form derivatives and sampled station cancellation do not prove
a uniform bound, regularity, contact compatibility or whole-face G2.

Remaining work, available independently of this preparatory slice:

- Discover/store crossing one-sided series and fix lambda from neighbouring
  stations before certificate refinement. Preserve maps, copies, transforms
  and definition persistence when introducing correction state.
- Verify the normal jump Taylor orders at both rails; give the designed
  refusal for insufficient face smoothness and name the order that builds.
- Enclose correction contributions uniformly in t, including the shrinking
  windows, and update definition evaluation, the net and its bound together.
  Gate that result-changing integration with an opt-in `UpstreamFixes`
  field under AGENTS.md section 4b; the default must retain pinned parity.
- Run the twisted-wedge and net-on-definition acceptance cases, rail contact
  and near-rail curvature measures. The minimal fixture here is not the
  complete capi/rig/ladder acceptance matrix.
- Stage 2: quintic Hermite net with triple knots, second derivatives and the
  sixth-derivative remainder certificate; enable its interior G2 gate.
- Inventory every changed refusal/result and re-derived volume for the
  owner before landing a production behavior change.

The separate arc-half seam tracked by #2330/WR-711 is not modified here.
The approved design cites Hartmann's rolling-ball contact construction;
its abstract was read, and this contribution follows #2356's specified
finite-smoothness correction rather than inventing another construction.

## Validation of the preparatory contribution

Source head tested: `e025c24a0c51c70e882080e4e8b35dcea6e7d60a`.
WSL Ubuntu, `CARGO_BUILD_JOBS=2`, all cargo commands under `nice -n 10`.
Shared pinned C++ libraries were reused without a kernel rebuild.

- `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed (default features).
- `cargo check -p solidcore --target wasm32-unknown-unknown`: passed.
- Optimized targeted tests: fillet_section 31, procedural_blend 8,
  issue_2356_smooth_interior 5, wr698_procedural_blend_definition 8;
  all 52 passed.
- Native Rust/C++ comparisons (dev profile, `--features cpp --jobs 2`):

```text
PASS blend.fillet_solver_p4 cases=651 passed=651 failed=0 deferred=0 max_rel_err=0e0 with_values=651 no_values=0 vacuous=0
PASS blend.fillet_standard_solver_p2 cases=525 passed=525 failed=0 deferred=0 max_rel_err=0e0 with_values=525 no_values=0 vacuous=0
PASS nurbs.b_spline_surface_n_lib_p1 cases=702 passed=702 failed=0 deferred=0 max_rel_err=0e0 with_values=702 no_values=0 vacuous=0
difftest: 3 unit(s), 3 passed, 0 partial, 0 failed
```

The native invocation used clippy-driver as `RUSTC_WORKSPACE_WRAPPER` to
reuse compiler artifacts; `RUSTC_WRAPPER` was unset because sccache is absent.
These comparisons validate selected default pinned behavior, not the new
fork-only smoothing construction. No full test suite or full `--green`
gate was run, and no unit status or tolerance was changed.