# Stress corpus S1983: the inward shell joins two previously separate rim loops

Investigation of SolidCraft issue #18803 on main `e319b8026` (the lane's claim
commit is `1c1a3e7c7`). S1983 needs boundary reconstruction when a periodic seam
disappears. Existing gap closure and curve continuation do not provide it.
This finding does not establish a shared cause for all 17 reported items.

This is the historical baseline investigation, before the boundary construction.
For the subsequent implementation and passing validation, see the
[periodic-boundary construction addendum](offset.offset_executive_p2--periodic-boundary-construction-18803.md#validation).
The regression is now a normal `#[test]`; `--ignored` skips it. The current
focused command, which runs both half-torus tests, is:

```sh
buildq run solidcraft . -- cargo test --manifest-path solidcore/Cargo.toml -p solidcore-ops --lib a_half_torus_ -- --nocapture
```

## Baseline reproduction

The public stress corpus's `items.jsonl`, item S1983, specifies an upper half
torus about Z, major radius 30 mm and minor radius 8 mm. A cylinder of radius
0.95 mm, centred at `(37.04998, 0)`, cuts upwards from Z = 0 to Z = 4. Shell
inwards by 0.8 mm with the annular Z = 0 face open. The later patterned holes
are unnecessary to reach the shell failure.

The recipe was available in a local checkout of the public corpus. Its prose
was translated into independent calls to SolidCore's torus, box, cylinder and
boolean primitives; no other CAD implementation was read or run.

The reproduction belongs to the existing
`crates/solidcore-ops/src/offset/tests.rs` test file:
`a_half_torus_with_a_blind_hole_shells_open_at_its_bottom`.
The test expects a valid shell and the independently derived volume below.
At the baseline revision, the test was explicitly ignored pending #18803;
`--ignored` selected it to reproduce the failure there. Both operand booleans
succeeded at that revision, then the shell failed with
`OffsetRefused { face: None, why: Phrase { class: StageFailed, defect: true,
phrase: "stage_failed" } }`. This differs from the report's older
`LoopGapUncertified` refusal; the test does not claim to reproduce that exact
message, or all 17 reported items. A diagnostic run with the application's
fix set (`test_corpus::rig_fixes`) confirms that the input passes
`CheckLevel::All`, then the kernel returns `StageFailed` from the
both-endpoints-away branch of `step_gaps.rs::close_gaps_of_faces` (line 473).

The same primitive recipe through the installed SolidCraft CLI 0.44.1 reproduces
the report's exact error at step 6 (`solid.shell`):
`the offset is refused: a gap in its offset's loops cannot be bridged within tolerance`.
This is additional reproduction evidence, not validation of the current checkout.
The captured bottom view and rim close-up were rendered by that CLI from the
five-step script before the shell, using `solidcraft-cli snapshot`; no changed
result was shown because the baseline investigation had no completed fix.

An independent reference construction succeeds in CLI 0.44.1: take the upper
half torus of minor radius 7.2, subtract a cylinder of radius 1.75 ending at
Z = 4.8 (the original hole's floor moved 0.8 into the material), then subtract
that cavity from the original drilled body. This is the set defined by the
analytic offset supports, not an alternative implementation added to Shell.
It builds one closed body with eight faces. A bottom-view image shows its
interior. Its measured volume is 3607.14296142513 mm³, agreeing with the
independent integral below; that measured number is not an oracle or a golden.

## Volume oracle

Let `T(q)` be the upper half torus of major radius `R = 30` and minor radius q,
and `C(b,h)` the hole cylinder centred a = 37.04998 from the axis, of radius b
and height h, starting at Z = 0. The cavity is
`T(7.2) \ C(1.75,4.8)`, contained in the original drilled body because
`T(7.2) ⊂ T(8)` and `C(0.95,4) ⊂ C(1.75,4.8)`. Subtracting its volume from
the original body's gives the shell's exact volume:

```text
π² R (8² - 7.2²) - H(8, 0.95, 4) + H(7.2, 1.75, 4.8),
H(q,b,h) = volume(T(q) ∩ C(b,h)).
```

At cylindrical radius ρ, the cylinder contains angles from -θ to θ, where
`θ = acos((ρ² + a² - b²)/(2aρ))`; the torus roof is
`sqrt(q² - (ρ-R)²)`. Thus, directly from cylindrical coordinates,

```text
H(q,b,h) = 2 ∫[lo,hi] ρ θ min(h, sqrt(q² - (ρ-R)²)) dρ,
lo = max(a-b, R-q), hi = min(a+b, R+q).
```

For both overlaps, split at `ρ = R + sqrt(q²-h²)`: below that radius the
height is h, and above it the height is the torus roof. Numerically evaluating
this derived integral gives

```text
H(8, 0.95, 4)       =  9.601995064515563 mm³
H(7.2, 1.75, 4.8)  = 16.313270972259300 mm³
shell volume       = 3607.14296142514 mm³.
```

For an independent numerical check, each interval was mapped by
`ρ = midpoint - halfwidth cos(t)`, t from 0 to π, and integrated by composite
Simpson quadrature. Counts 4096, 8192, 16384 and 32768 agreed in the shell
volume to 5e-13 mm³. This convergence check is not an interval certificate.

## Exact change of topology

On Z = 0 the torus's outer rim has radius 38 and the hole has radius 0.95
about X = 37.04998. Their separation is

```text
38 - (37.04998 + 0.95) = 0.00002 mm (20 nm).
```

The inward torus offset has minor radius `8 - 0.8 = 7.2`, hence outer rim
radius 37.2. The hole's offset wall has radius `0.95 + 0.8 = 1.75`.
These circles intersect because

```text
37.04998 - 1.75 < 37.2 < 37.04998 + 1.75.
```

Subtracting their equations, with `a = 37.04998`, `R = 37.2` and `r = 1.75`,
gives two new vertices on the open plane:

```text
x = (R² + a² - r²) / (2a) = 37.158974417805353741...
y = ±sqrt(r² - (x - a)²) = ±1.746602478209415569...
z = 0.
```

The original torus and cylinder first meet above this plane at
`z = sqrt(8² - (37.04998 + 0.95 - 30)²) = 0.017888532639654936...`.
Thus the new vertices are not the offsets of existing shared rim vertices:
the shell must resolve a meeting of initially separate loops.

There is also a disappearing periodic seam at Y = 0. Originally the outer
meridian has a short boundary stretch from `(38,0,0)` to
`(37.99998,0,0.017888532639654936...)`, where the drill starts cutting it.
The offset cylinder's outer meridian is X = 38.79998, beyond every point of
the offset torus (X <= 37.2). Its inner meridian is X = 35.29998, where the
offset torus reaches Z greater than 4.8, beyond the offset hole's floor.
Thus neither cylinder meridian meets the torus on the cylindrical wall's
Z range. The remaining upper torus meridian meets the floor plane,
at `Z = 4.8`, `X = 30 + sqrt(7.2² - 4.8²) = 35.3665631459995...`.
The lower seam stretch disappears; the surviving upper meridian starts at
the offset cap.

Temporary endpoint tracing, removed after the diagnostic test, connects this
event to the refusal on offset face 0. Shared vertex 2 has three incident
edges (1, 2 and 4):

| entity | endpoint or shared point, mm |
|---|---|
| edge 4, positive-Y torus/cylinder branch | `(37.15895702855769, 1.7466036071772586, 0.015786403528057026)` |
| edge 2, lower meridian seam | `(37.199982, 0, 0.01609967937580376)` |
| shared vertex 2, on the negative-Y branch | `(37.15895702080245, -1.7466035664037523, 0.015795417119325913)` |

The gap search's existing tolerance is 0.0002 mm. On edge 4's whole stored
curve, closest-point search gives distance 3.4932071735926398 mm from the
shared vertex; on edge 2 it gives 1.747085319680026 mm. Each edge's natural
interval equals its stored trim interval. The seam is an exact ellipse, the
torus meridian `(X-30)² + Z² = 7.2²` in Y = 0. No continuation of it can reach
the shared vertex: its Y coordinate alone differs by 1.7466035664037523 mm.
Thus retaining a longer parameter interval cannot repair this incidence.

Nor can continuing the positive SSI branch to the negative one repair it
inside the active pocket wall. A continuous connection must cross Y = 0.
There the cylinder requires X = 38.79998 (outside the torus) or X = 35.29998,
where the upper torus is at
`Z = sqrt(7.2² - (35.29998-30)²) = 4.873418923055971... > 4.8`.
That connection would pass above the offset floor and leave the cylindrical
face. The original one shared seam vertex must be replaced by distinct
surviving vertices, including the new open-plane intersections above.

## Construction investigation

`offset_geometry_creation.rs`, `trim_extended_edge`, chooses trim ends from
the original vertex's neighbouring edges and surfaces. The extended-offset
executive processes mapped original edge/radial pairs, then calls
`close_loop_gaps` before its piecewise merge. `step_gaps.rs`, `close_loop_gap`,
can trim an edge at a vertex already on its curve, or add a straight segment
on a face; it does not construct the new intersections of separate loops.
The reported `LoopGapUncertified` is the straight segment's refusal when its
interior does not lie on the face within the existing tolerance.

The existing `resolve_crossing_loops` handles an inner loop growing past an
outer loop, after gap closure. It cuts existing trimmed UV curves
(`crossing_loops.rs`, lines 728–767), assigns consecutive endpoints the old
synthetic vertex nodes (lines 862–868), and classifies those stretches using
loop containment. It cannot generate a missing boundary from this open trim
graph. Running it before gap closure would violate those connectivity and
containment assumptions. The endpoint diagnostics above rule out a curve
continuation or vertex reassignment as the complete fix.

The minimum missing construction must detect and remove the obsolete lower
seam, intersect surviving boundaries with the open plane and pocket cap,
replace the original seam vertex with the distinct surviving vertices, and
rebuild periodic UV loops sharing their resulting 3D edges. Existing gap
closure has neither those intersections nor that connectivity reconstruction.

The torus's offset itself retains its UV parameterisation: `Torus::create_offset_surface`
clones the torus and changes its control points, leaving its knots and domain.
The existing `offset_follows_the_nurb_normal_in_every_orientation` test specifies
the same-parameter offset identity. No torus parameterisation change is justified
by this reproduction.

`docs/offset-and-shell.md`, sections 5.1 and 9, records the general offset-sheet
arrangement, winding-number cavity rule and boundary extraction as open work.
This reproduction supplies a concrete event that the boundary construction
must handle. It does not establish that the complete general arrangement
redesign is necessary, or that every case in #18803 has the same cause.

No tolerance increase, straight chord on a curved face, retry or alternate
construction is justified. No production geometry or golden is changed by this
baseline investigation. At that stage, implementation was blocked on the missing
disappearing-seam boundary construction; the pending regression and analytic
oracle preserved the evidence without accepting an invalid shell. The addendum
records the subsequent construction and validation.

## Baseline validation

The commands and statuses below describe the baseline investigation, before
the construction in the addendum. All Cargo commands ran from the worktree
root through `buildq run solidcraft . --`.
The first focused regression failed before any production edit, with the
`OffsetRefused` / `StageFailed` quoted above. The corrected endpoint diagnostic
also failed there; its temporary tracing is removed from both source files.

The final, clean regression was explicitly included in the existing offset
module's tests:

```sh
buildq run solidcraft . -- cargo test --manifest-path solidcore/Cargo.toml -p solidcore-ops --lib offset::tests:: -- --include-ignored --skip a_box_with_a_lofted_tunnel_shelled_passes_the_full_check
```

Result: **42 passed, 1 failed**, exit 101. The sole failure is
`offset::tests::a_half_torus_with_a_blind_hole_shells_open_at_its_bottom`, at
the shell call, after its input-validity assertion passes. The unrelated
pre-existing ignored #17608 full-check test was excluded explicitly. The
pending #18803 test was ignored in normal runs at the baseline revision; this
historical command included it. The current test has no ignore marker; use
the normal command above and the addendum's validation results.

These checks passed (exit 0):

```sh
buildq run solidcraft . -- cargo fmt --all
buildq run solidcraft . -- cargo fmt --manifest-path solidcore/Cargo.toml --all
buildq run solidcraft . -- cargo clippy --manifest-path solidcore/Cargo.toml -p solidcore-ops --all-targets -- -D warnings
git diff --check
```

The baseline investigation changed no production source or golden. Full suites,
wasm and goldens remained for CI under the lane's test policy. This historical
record does not claim a completed fix; see the addendum for the later result.
