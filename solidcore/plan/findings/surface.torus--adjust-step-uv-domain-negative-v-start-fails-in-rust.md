# surface.torus: AdjustSTEPUVDomain fails in Rust when the V range starts at or below -180 (2026-10-25)

Found by `difftest --step-corpus --constructor` (PR #1000, unit `topology.brep_constructor`).
The importer recipe called `SmSurface::AdjustSTEPUVDomain` with each face's STEP parameter box.

**Inputs where the sides disagree.** Tori with a V (tube angle) range that starts at or
below -180:
- `ap203/moon_buggy_asm` body 8: `[0, -270, 360, -90]`.
- `ap203/weldment_asm_solid` bodies 2 and 3: `[180, -270, 360, -90]`.
- `ap203/rear` body 29: `[0, -180, 180, 0]`.

C++ returns `SM_SUCCESS`. Rust returns `SM_ERR`, raised from
`nurbs::b_spline_surface::p1::BSplineSurface::create_canonical` while
`SurfOfRevolution::adjust_step_uv_domain_using` rebuilds the NURB. This is a Rust-side
defect: upstream documents the range as `-360 <= min <= max <= 360`.

**Reproduction** (Rust only, `crates/difftest/examples/torus_adjust.rs` on the PR branch):
- `Torus::create_canonical(frame, 177.8, 12.7)` fails with `[0,-270,360,-90]` and with
  `[0,-180,180,0]`.
- The same tori succeed with `[0,90,360,270]`, `[0,180,180,360]`, `[0,-90,360,90]` and
  `[0,-100,90,-80]`.

**Suspected area.** Converting the generator circle's STEP interval to NURB parameters
(`Ellipse::adjust_step_interval` / the polar converter) for starts at or below -180 in
the torus's `MakeNurb`. Not investigated further.

**Related case.** `ap203/weldment_asm_solid` body 0, `[-180, -270, 0, -90]`: C++
segfaults inside `AdjustSTEPUVDomain` while Rust returns `SM_ERR`.

**Workaround in the recipe.** `step_import::script::step_uv_domain` now moves each
periodic range to start in `[0, 360)`, so the corpus no longer reaches either case.
Owner units: `surface.torus` and `surface.surf_of_revolution_p2`.
