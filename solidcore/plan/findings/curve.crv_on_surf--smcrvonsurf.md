# curve.crv_on_surf (SmCrvOnSurf)

### Concrete Surface integration follow-up

`CrvOnSurf` now retains the existing concrete `Surface` rather than a B-spline
part. Native owning `Copy` branches still call the virtual geometry copy; shared
UV curves, surfaces and temporary evaluation snapshots instead retain exact NURBS
data, ownership/face chains and caches. Snapshots are taken at the original
borrow, before any ordinary clone can repair close knots. The non-owning
`OffsetSurface::copy` branch uses the same exact shared-base semantics. Native
temporary owner clearing remains explicit and propagates through owned bases.

Both UV-curve and surface children are checked against the existing 64-level
geometry bound before construction, domain queries or copying. `empty` and
`set_uv_curve` now return `Result`; a rejected replacement leaves geometry and
caches unchanged. Transforms carry the actual enclosing depth through compound
curves/surfaces to volume maps: only a terminal-map append consumes another level.
At the boundary, transform absorption and identity still work, while a rejected
append retains the earlier native approximation-cache edit. Direct depth,
owner-chain, borrowed close-knot lift/approximation and transform-boundary
regressions exercise these contracts.

The native fixture transport builds real `SmOffsetSurface` objects, including
borrowed raw surface and UV NURBS, and compares typed state plus numeric evaluation.
Thirteen offset-backed fixtures passed bit-for-bit: signed planar/curved
evaluation, derivatives and normals, supplied-knot approximation/reuse, owner
flags 0–3 crossed with borrowed/owned offset bases, borrowed close UV knots, and
failing transforms with post-error state. These comparisons do not certify
arbitrary returned offset B-reps or live cross-object alias mutation. The broader
analytic-kind dispatch work in #76 and the production executive/SSI bridge remain
separate.

### Final local integration evidence

The final source, including canonical batch #305 (`bb137d7d`), passed all 44
affected native runners: 111,195 cases, 102,648 passing, zero failed, 8,529 deferred
and 18 native setup rejections.
The largest relative error was `1.7805513589745123e-13`; every passing case's
perturbation was detected. No manifest-green unit regressed to partial.

| Runner | Cases | Passed | Deferred | Remaining limitation |
|---|---:|---:|---:|---|
| `curve.crv_on_surf` | 2107 | 1800 | 307 | Uncached classification breaks in `intersect.curve_class_p2` |
| `blend.fillet_intersector` | 1286 | 1037 | 249 | Fillet solver parts 1–4; 162 former concrete-surface deferrals now pass |
| `offset.offset_geometry_creation` | 861 | 428 | 433 | Face ownership/tolerance, B-rep and cache dependencies remain |
| `intersect.curve_class_p5` | 1081 | 1081 | 0 | Owned SSS/edge-target insertion remains unported and unexercised |

The last row is fixture coverage, not a green promotion: its manifest status stays
partial. In the offset creator, 25 former concrete-surface deferrals now reach
`topology.face_p2`; the `box_edgeuse_offset_shell` smoke observes the real
owner/tolerance dependency rather than a fabricated standalone tolerance.

The remaining uncached-break `ReSet` label is an interface gap, not an algorithm
to re-port: classification reset is available, but `Curve::knots` does not carry
the caller's `Kernel`. A separate cutover must thread that real context through
the query/constructor chain, including `CrvInVolume`; a fresh default kernel
would not preserve caller tolerances. PR #137 explicitly leaves these
Kernel-less sites outside its reset wiring, and PR #301 lists the same gap as a
follow-up. Face-tolerance caching itself is already claimed by #301.

Workspace tests passed (1,533 kernel tests), as did formatting, strict workspace
Clippy both with and without C++, and the wasm32 check. Independent reviews
explicitly selected `openai-codex/gpt-6-astra`; the final snapshot/owner/transform,
fuzz-safety and batch-merge construction deltas received no blockers. The merge
keeps main's owning-UV flag 1 together with the concrete-Surface edgeuse import.
Canonical x86 full-green CI is the publication integration gate; the selected ARM
comparisons above do not replace it.

#### Integration with main `ca3bbd2b` (2026-10-21, PR #307)

The incoming `FindVertexSector` UV lift now uses evaluation snapshots of the actual
face surface and UV curve; a Rust regression covers the newly supported plane
surface. The native fixture still passes a 3D curve, so it does not certify that
UV-lift branch.

The combined face-p4 wiring and concrete-surface support exposed eight disk
`CreateUVTrimCurves` fixture mismatches. The native runner constructs
`new SmBSplineCurve(*c)`, slicing an analytic UV `SmLine` to its B-spline base;
the Rust runner instead cloned its dynamic `Line` kind. Its analytic evaluation
and the native basis-weighted sum differ by rounding, producing maximum gaps of
`5.200132600947524e-16` and `6.718298285881963e-16`, respectively. Copying the
B-spline base in the Rust runner fixes all eight without changing production
numerics, comparisons, tolerances or fixtures. The full face-p3 rerun has 1,781
cases, 843 passes, 938 deferrals and zero failures; sensitivity detects 843/843.

The five conflicted offset/topology reports were regenerated, not selected from
either merge side. The offset creator now has 874 cases, 585 passes and 289
deferrals, matching current main's aggregate counts; its 28 concrete-surface rail
deferrals instead reach the actual face-owner/tolerance dependency. Partial
implementation boundaries remain unchanged.

### Rust safety bounds and malformed-stream fuzz

The final fuzz run exposed caller-sized derivative allocation and unbounded
approximation work. `CrvOnSurf::evaluate` now limits requested output order to
4096, with checked size arithmetic before allocation; it still evaluates at most
three derivatives, preserves earlier geometry errors and fills accepted higher
orders with zeros. The common Hermite approximation permits 8192 adaptive
bisections per call, preserving the native depth-first order and formulas rather
than rejecting, sorting or clamping supplied breaks. A non-finite/non-progressing
midpoint or an infinite computed tangent returns an error instead of looping.
These are documented Rust safety deviations, not native-verified status codes for
inputs outside the existing FFI oracle domain.

| Fuzz corpus | Cases | Rejected | Malformed | Ran | Differs | Crashes | Panics | Timeouts |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Offset fixtures, before bounds | 1000 | 488 | 205 | 289 | 0 | 5 | 1 | 12 |
| Offset fixtures, after bounds | 1000 | 506 | 205 | 289 | 0 | 0 | 0 | 0 |
| Unfiltered, after bounds | 1000 | 394 | 326 | 280 | 6 | 0 | 0 | 0 |

The fuzzer still exercises Rust after a C++ harness error; the saved extreme
derivative/break requests were rejected by the existing native guards before
geometry operations ran. No guards, tolerances, expected outputs or original
inputs were changed to obtain these results.

The six unfiltered differences reproduce the existing malformed-binary-reader
safety deviation described below, not accepted-record parity: `fuzz00062`,
`fuzz00070`, `fuzz00511`, `fuzz00516` and `fuzz00762` empty/truncate database bytes
(`SM_ERR` 1001 upstream versus bounded-read `InvalidInput` 1007); `fuzz00322`
duplicates byte 615 and is rejected because its header-declared size exceeds the
remaining input, while C++ emits state (176 output values versus Rust's two-value
error). A direct Rust reader probe confirmed those exact rejection reasons.
Malformed-record status/state equivalence is not claimed, and the mandatory
input-size protections remain unchanged.

### Historical original-port notes

- **Partial, not blocked.** Every non-deferred difftest case is bit-identical
  (max_rel_err 0, `--perturb` SENSITIVE). The deferred cases reach: the
  `SmCurveClassification` of uncached breaks (`intersect.curve_class_p4/p8/p1`, behind
  `GetKnots`, `GetNumberNaturalKnots`, `CalculateContinuities` and the approximation
  without given breaks), `SmSurface::EvaluateNormal` (`surface.surface_p7`: the surface
  normal and every sample of `SurfaceCurveMaxDistanceBetween`),
  `SmSurface::Evaluate2ndDerivatives` (`surface.surface_p1`: `EvaluateSurface1stDerivDerivs`)
  and `SmSurface::ReadFromDB` (`surface.surface_p2`: every complete `ReadFromDB`
  record), plus a few `curve.curve_p5` solver callees.
- **`GetKnots` with `pOptIvl`** on an uncached curve reads `rKnots[0]` after asking the UV
  curve for its knots in the interval: an interval holding no UV knot reads an empty
  array (a crash in the harness). It also reads `GetAt(lCnt - 1)` when the classification
  has no interval. The port returns `InvalidInput`; the ffi only passes intervals holding
  an end of the unedited UV curve. The cached path ignores `pOptIvl` and returns every
  break (multiplicities `4, 2, ..., 2, 4`); the uncached path caches the interval's
  breaks only.
- **`SurfaceCurveMaxDistanceBetween`** sets `bDoPrecise = FALSE` unconditionally before its
  precise refinement, so the `SmSurface::LocalCurveSolve` / `LocalCurveSolve` code after
  it is dead (not ported). With `lNumSamples == 1` it divides `0.0 / 0.0` (the ffi rejects
  one sample). `GetDegree` is not overridden (3), so the line/line special case never
  applies.
- **`EvaluateSurface1stDerivDerivs`'** zero fill (`for (ii = 2; ii > lNumDers; ii++)` inside
  `if (lNumDers > 1)`) never runs; only the first two derivatives are written.
- **Caches through `const_cast`.** `GetKnots`, `GetOrCreateBSApproxPointer` and the const
  methods calling them update the caches; `Trim` drops the breaks but keeps the B-spline
  approximation (a later `GetOrCreateBSApproxPointer` without breaks reuses the stale
  approximation when its gap is small enough), `EditParameterization` rescales the
  breaks only, `ReverseParameterization` touches neither. The port keeps all of this;
  the `Curve` dispatch (`&self`) runs these methods on a copy.
- **Binary records cut short.** A binary `ReadFromDB` record truncated inside the UV
  curve's B-spline data gives `SM_ERR` upstream (after reading garbage counts) and
  `InvalidInput` here (counts bounded by the bytes left, as "header-declared sizes"
  above); the generator damages records by replacing the UV curve type instead.
- **Stubs left.** Three `UNPORTED(curve.crv_on_surf)` sites cannot be wired yet:
  `geom/global_solver.rs` (`SmCrvOnSurf::EvaluatePoint` on a tree node's owner object,
  which is only an `ObjectRef` handle here; the solver would have to carry the curve, as
  `CpGlobalSolver::curve` does), `nurbs/b_spline_surface_n_lib/p2.rs`
  (`SmTangentField::ApproximateCurve`, inherited from `SmCrvOnSurf`: the tangent field is
  not a `Curve` kind and `Curve::approximate_curve_base` is not generic; its evaluation
  also needs `surface.surface_p7`) and the `SmTangentField` arm of `SmCurve::ReadFromDB`
  in `curve/curve/p2.rs` (no tangent-field curve kind; unreachable for now: the
  `SmCrvOnSurf` part of the record defers on `SmSurface::ReadFromDB` first). The
  `DeformToEndPointTargets` branch for curves on surfaces is retagged
  not ported (curve_p6's own limit, no longer tagged to surface.surface_p5): it is taken only with the
  `pOptSurfCrvOnSurf` argument the port drops, and needs `SmSurface::DropPoint`.
- **Wired.** `SmCurve::ReadFromDB(SmCrvOnSurf_TYPE)` (curve.curve_p2) and the
  `SmCrvOnSurf` part of `SmTangentField::ReadFromDB` (curve.tangent_field: its 2
  `curve.crv_on_surf` deferrals now pass; that read's status is ignored upstream).
  `ApproximateCurve`, `DropPoint` and `CalculateBoundingBox` now dispatch to the
  `SmCrvOnSurf` overrides (`Curve::approximate_curve_base` / `Curve::drop_point_base` keep
  the base bodies). `smsurf_LiftSecondDerivative` / `smsurf_LiftThirdDerivative`
  (`surface.surface_p2`) are co-ported in `curve/crv_on_surf.rs`.
