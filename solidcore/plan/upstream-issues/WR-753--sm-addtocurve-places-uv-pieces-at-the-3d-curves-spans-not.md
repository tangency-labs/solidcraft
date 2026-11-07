#### WR-753. `sm_AddToCurve` places a dropped UV curve's pieces at the 3D curve's spans, so a surface knot line inside a span becomes a curvature spike

- **Severity:** Wrong result (a valid body refused: a UV trim's radius of curvature is read as a
  kink where the trim's image is smooth to a fraction of the edge's tolerance)
- **Area:** Booleans, surfaces and volumes (surface drop library)
- **Location:** `source/SMLib/src/SmSurfaceDropLib.cpp:3439`, `sm_AddToCurve`, and its caller's span
  walk `DropCurve` (`:5621` region), at v1.0.1 (`3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5`; the
  port is `crates/solidcore/src/intersect/surface_drop_lib/p2.rs`, `add_to_curve`, and
  `p3.rs`, `drop_curve_base_in`).
- **What happens:** the drop walks the 3D curve's own spans and emits one cubic Hermite Bézier
  piece per span, with `dScale = crEnd.m_dT - crStart.m_dT` (`:3459-3463`). The pieces' breakpoints
  are therefore the 3D curve's knots. A surface that is only C0 across a knot line — a loft through
  sections is C0 in v at every section, its `|S_v|` jumping there — has a discontinuous tangent
  plane on that line, so the dropped UV derivative (`m_UVPD[1]`, the 3D derivative projected onto
  the offset surface's tangent plane) jumps across it. When the surface's knot line falls strictly
  inside a 3D span, no cubic piece can carry that jump: the piece's interior control point is
  placed from the derivative at the span end, which is enormous next to the span's UV chord, and
  the piece loops. The image `S(u(t), v(t))` then has a curvature spike — a notch a fraction of
  the edge tolerance deep — although the 3D curve is smooth there. Nothing in the drop checks
  whether a surface knot line lies inside the span it is about to emit.
- **Trigger:** `crates/solidcore/tests/kernel_cases/crashes/2992-join-disk-and-36-twisted-loft-
  blades` (SolidCraft's `crashes/2992`), `union_all` call 111. Its 36 blades are lofts whose
  surfaces are C0 in v at every integer v (`v_knots` `0,0,0,0, 1,1,1, 2,2,2, … 19,19,19,19`), and
  the intersection curve's spans end within `2e-5` of those lines but not on them.
- **How found:** tangency-labs/solidcore#2785 (the radius of curvature is a length, so tests 13/14
  compare it with a length; `plan/findings/nurbs.b_spline_curve_p3--issue-2785-radius-tolerance-is-
  a-length.md`). The check's report on that case was traced to a UV trim whose image has a
  curvature spike where its 3D curve does not.
- **Confirmed:** yes, on the port, from the union's result body (185 faces, 549 edges, 366
  vertices, the corpus's raw counts). One trim of it (`[0, 61.38314]`, 121 control points, 41
  distinct knots, every interior knot of multiplicity 3 — one Bézier piece per 3D span — on a loft
  whose `v_knots` are the integers 0…19), at its smallest image radius:

  | offset from the spike, x the trim's domain | `\|C'\|` of the image | image radius | the 3D curve's radius | image − 3D curve | tangent, image vs 3D |
  |---|---|---|---|---|---|
  | −1e-6 | 1.000000 | 75.244 | 75.244 | 3e-14 | 1e-12 rad |
  | −1e-7 | 1.000000 | 3.976e-4 | 75.434 | 5e-14 | 1e-10 rad |
  | −1e-8 | 1.142142 | 6.566e-4 | 75.402 | 4.0e-7 | 1.16e-2 rad |
  | −1e-9 | 0.715812 | 2.652e-4 | 75.379 | 2.9e-7 | 3.70e-2 rad |
  | 0 | 0.716720 | **2.665e-4** | 75.389 | 2.7e-7 | 3.68e-2 rad |
  | +1e-7 | 0.802034 | 4.254e-4 | 75.368 | 1.2e-6 | 2.30e-2 rad |
  | +1e-6 | 1.071769 | 4.052e-3 | 75.004 | 5.5e-7 | 6.2e-3 rad |
  | +1e-5 | 1.000000 | 75.255 | 75.431 | 5e-12 | 1.7e-8 rad |

  The spike is at the last of the trim's knot cluster `54.943193, 54.948888, 54.948969` (each of
  multiplicity 3), and the trim crosses the surface's `v = 12` line there; the 3D curve's radius is
  75.4 on both sides and its speed is 1 throughout. The image's position error peaks at `2.7e-7`
  mm against the edge's `2e-5` mm tolerance, and its tangent turns `3.7e-2` rad and returns within
  `1e-5` of the domain — the image is smooth to well inside the model's tolerance, and only its
  second derivative is not. On the same body, 184 pcurves have more than 40 knots and 1898 UV trims
  were scanned; the smallest image radius over all of them is `1.71e-4` at a knot cluster of the
  same shape, against a 3D radius of `234.2` there.
- **Rust port:** faithful to the pinned C++ (`add_to_curve` reproduces `sm_AddToCurve`'s
  `dScale`-scaled Hermite span, `flush_curve` its `[4 3 … 3 4]` piecewise-Bézier curve), so the
  notch is upstream's. Not fixed here: the fix is to break the drop's pieces at the surface's knot
  lines (so no piece straddles a C0 line), which moves every pcurve that crosses one and needs its
  own sweep. The radius test on a trim reports it as a kink (tests 13/14, `SmBSplineCurve::
  AssertValid`), which is why `Kernel::fixes.bspline_check_min_radius` (WR-382) is off by default
  and why the kernel rig, which turns it on, refuses this case.
