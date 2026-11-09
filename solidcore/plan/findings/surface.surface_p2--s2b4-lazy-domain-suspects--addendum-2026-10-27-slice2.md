# surface.surface_p2: S2b-4 slice 2, `SmIsoCurve` clamps to and spans the made NURB's domain (2026-10-27)

Updates: plan/findings/surface.surface_p2--s2b4-lazy-domain-suspects.md

Slice 2 covers the `curve/iso_curve.rs` sites.

- **`IsoCurve::from_surface`** (`SmIsoCurve(crSurface, .., bClampInput)`) now takes the kernel. With `clamp_input` it clamps to `natural_uv_domain_with_nurb(k)` (upstream's `GetNaturalUVDomain`). The domain is read before construction, after the nesting check.
  - Callers given `k`: `blend/fillet_solver/p3.rs` `iso_curve_of`, `blend/fillet_solver/p1.rs`, `surface/surface/p1.rs` (`approximate_size`), `p5.rs` `iso_curve_of`, `p6.rs` `boundary_iso_curve` (from `p11.rs`, `p6.rs` `sc_local_solve` and `p8.rs` `intersect_srf_bndry_crv`), `p9.rs` `implicit_iso`, `surface_point_solve/p3.rs`, `IsoCurve::read_from_db`, the `curve.iso_curve` runner and tests.
  - Every caller already held a kernel.
- **`IsoCurve::create_uv_iso_line_with_nurb(k)`** (`CreateUVIsoLine` over `GetNaturalUVDomain`) is new. `Surface::create_iso_parametric_curve` (`surface/surface/p5.rs`) and the runner use it.
  - The kernel-free `create_uv_iso_line` stays for `Plane::create_iso_parametric_curve` and `BSplineSurface::create_iso_parametric_curve`. Their iso curves hold a B-spline (`IsoCurve::new`), which is never lazy.
  - Threading `k` there would only forward it through the plane's kernel-free API.
- **`IsoCurve::new`** holds a B-spline. It reads the built domain, unchanged.
- **Not changed:** `IsoCurve::natural_interval` (`GetNaturalInterval`) stays kernel-free. It is reached through the kernel-free `Curve::natural_interval` dispatch, the same blocker as the generic `SurfaceEvaluate` paths.

Test: `crates/solidcore/tests/lazy_analytic_nurb.rs` compares, for each lazy kind against the made one, a clamped iso curve past the u domain and its UV line.
