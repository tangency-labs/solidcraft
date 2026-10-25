# blend.fillet_intersector `rv_next_clipped_step`: ULP-level next point once `ComputePointValues` is wired (2026-10-22, open)

Found while landing `blend.fillet_solver_p4` (#212, batch/finish-3, draft PR #564). With
`ComputePointValues` wired (and `curve.crv_on_surf` landed, #554), the fixture case
`rv_next_clipped_step` (`fixtures/blend.fillet_intersector/review_cases.json` #72: planes
`z = 0` and `y = 0`, domains `[-1, 1]^2` beyond the planes' `[0, 1]^2`, radii 0.1,
`ComputeNextPoint` from uv `(0.9, -0.1)` / `(0.9, 0.1)` with step 0.5, clipped) no longer
defers. Every output agrees within `1e-9` except the found angle `rec[3]`: C++
`3.331303480692949e-16`, Rust `3.3306690738754696e-16` (rel. 1.9e-4; both are noise, the
angle between nearly parallel tangents). The bit-level differences are in the next point that
`StepSolve`'s clipped iso-curve branch returns: `rec[115]` C++ `-9.999999999999992e-2`, Rust
`-1e-1`; `rec[123]`, `rec[177]` C++ `-9.999999999999987e-2`, Rust `-1.0000000000000009e-1`.
The same disagreement shows with `GetSolverType` either `SM_FS_CONST_RADIUS` or
`_ASSISTED`, so it comes before the assisted refinement: `SmIsoCurve` over the offset surface and
`SmSurface::LocalCurveIntersect` on the other offset surface, both evaluated outside the planes'
domains (extended bases). Suspected cause: the extension state of the solver's offset surfaces.
Upstream evaluates the solver's own `SmOffsetSurface`s, and their `m_pExtendedSurface` grows with
each out-of-domain evaluation. Rust's `blend::fillet_solver::p3::step_solve` evaluates clones
(`iso_curve_of`, `other`), so extensions built there are dropped. A Newton solve that starts
from a differently extended base can converge to a point a few ULP away. Not confirmed:
confirming it needs a C++ probe of `LocalCurveIntersect` on a shared vs a fresh offset surface.
Not fixed, and no tolerance was loosened. The case is listed in
`crates/difftest/known_mismatches.toml` (reported as `known-mismatch`; `blend.fillet_intersector`
stays partial, never green). Remove the entry when the cause is fixed.
