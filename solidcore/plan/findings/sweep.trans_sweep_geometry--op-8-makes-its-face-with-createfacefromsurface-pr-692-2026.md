# sweep.trans_sweep_geometry: op 8 makes its face with CreateFaceFromSurface (PR #692, 2026-10-23)

The `FaceSweepSame` op (op 8) used to build its input face differently on each side: the
shim calls `SmBrep::CreateFaceFromSurface(surface, natural domain)` on a `new SmBrep()`,
while the Rust runner pushed a bare face record (no loop, no infinite region) and set its
surface by hand. When `CreateFaceFromSurface` failed, the shim returned `SC_ERR_ARG` and
the case was recorded as a rejected "native fixture constructor", so 11 cases never
compared anything. All 11 failed with `SM_ERR` (1001) inside `MakeFaceWithCurves`: the
B-spline extrusion of a 1e-9-scale or near-degenerate profile curve degenerates to a
curve (`nanoscale_fixture_{0,1,2,4,5}_8`, `gen0147`, `gen0256`, `gen0337`, `gen0386`,
`gen0405`, `gen0471`).

Now the Rust runner makes the face with `topology::brep::p8::create_face_from_surface`
on `topology::brep::p3::new_brep(k, USE_DEFAULT, USE_DEFAULT)` (the B-rep needs its
infinite region, as `new SmBrep()` has), and the shim returns a failed
`CreateFaceFromSurface` status as the case's output instead of rejecting. All 11 cases
compare `[1001]` on both sides. No FFI guard was removed: `SC_ERR_ARG` is still
returned when the profile curve constructor fails, when the profile is not a
`SmBSplineCurve`, when `SmBSplineSurface::CreateLinearSweep` fails, and when
`CreateFaceFromSurface` succeeds without a face.

- **Still rejected (op 8):** `rejected_constructor_0_8` and `rejected_constructor_2_8`.
  Their profile curve constructor fails (a 1e-15 line segment, a 1e-15 radius circle)
  before any face exists, exactly like their op 5-7 siblings, so the sweep is never
  reached. The unit keeps 8 rejected fixtures, all curve-constructor failures.
- **New deferral:** `gen0411` (op 8, a circle profile of radius about 1.5e-6 extruded
  into a closed B-spline surface) passed before, against the bare face record, and now
  defers on `topology.face_p2` ("SmFace handle of a surface's face",
  `surface/surface_cache/p1.rs` `SurfaceCache::face`). Natively `CreateFaceFromSurface`
  succeeds. In Rust, `MakeFaceWithCurves` splits the closed face (`split_closed` ->
  `find_and_set_infinite_region` -> `find_infinite_region`), which ray-fires the shell;
  the line/surface solve on the now face-owned surface builds a trimmed surface cache
  that needs the owning face's caller handle. That is the shell ray-fire gap that the
  open face-geometry-for-shell-ray-queries work addresses. It is the unit's only
  deferral, so the unit goes from green to partial.

Before: 788 cases, 769 pass, 0 deferred, 19 rejected. After: 788 cases, 779 pass
(the 11 re-covered cases included), 0 failed, 1 deferred (`gen0411`), 8 rejected,
max_rel_err 0.
