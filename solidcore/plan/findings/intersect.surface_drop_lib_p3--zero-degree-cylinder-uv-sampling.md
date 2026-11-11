# intersect.surface_drop_lib_p3: zero-degree cylinder UV sampling panics (2026-11-05)

- Unit: intersect.surface_drop_lib_p3.
- Issue: #2777. PR: #2778.
- Location: cylinder_section::join in
  crates/solidcore/src/intersect/surface_drop_lib/cylinder_section.rs.
- Inputs: default Kernel with cylinder_plane_section_trims_exact enabled; cylinder
  at (0,0,0), X/Y axes, radius 16, angles 0..360 degrees, height 20, natural UV
  domain; NurbCurve::new with degree 0, one homogeneous control point
  (12.8,9.6,10,1), knots [0,1], wrapped by
  BSplineCurve::from_nurb(nurb,3,false); public Surface::drop_curve_base over
  [0,1], tolerance 1e-6, keep_all=false, no failure record.
- Observed: flag off returns SolidError::Upstream, operation
  intersect::surface_drop_lib::Surface::drop_curve_base, code 1006. Flag on panics
  with assertion failed: step != 0. A captured backtrace reaches exact_uv_curve
  from the public drop_curve_base_in call.
- Cause: the constant UV piece has one control point. join derives degree 0 and
  passes that degree to Iterator::step_by before its non-iso eligibility check.
  A constant curve is outside that exact construction's intended eligibility.
- Scope: this is a Rust-owned construction and a Rust panic. No C++ behavior for this degree-zero input,
  upstream defect, numerical tolerance change or successful constant-curve drop
  is claimed. The ordinary drop's existing refusal remains visible.
- Reproduction: an external Rust executable calls the real public API with the
  inputs above; it does not copy the construction. Optimized kernel artifact
  SHA-256: 799a5873e94372fd4b16cef6812097d11d67bafebe3fc18fd16a8714d39d0a41,
  after the main 4b10d7d6e build. At reproduction time, this source file matched\n  f4e14b6cf.
- Tests first: the actual optimized test build completes, then both new
  existing-module regressions panic at step_by(0). The same filter retains two
  passing degree-zero controls elsewhere in the kernel.
- Correction: join declines zero-degree pieces before allocating its output or
  sampling endpoints. The existing normal drop retains status 1006. Arithmetic,
  eligibility tolerances and sampling order for positive degrees are unchanged.
- Evidence: all four cylinder-section tests pass, including the existing exact
  joint controls. The full optimized kernel library passes 3,531 tests with zero
  failures and four existing ignores. Wasm and formatting pass. The existing C++
  differential unit passes all 1,532 cases, all with values, max_rel_err=0.
  These existing C++ cases do not establish reference behavior for the new
  degree-zero input. Both new regressions use Rust APIs only.
- Final validation: after replacing the new test's redundant closure with the
  same method pointer, the optimized kernel library again passes 3,531 tests,
  zero failures, four existing ignores (75.07 seconds). Strict workspace clippy
  with all targets, wasm and formatting pass. The C++ run above predates only
  that test-only cleanup; production source is unchanged by the cleanup.
