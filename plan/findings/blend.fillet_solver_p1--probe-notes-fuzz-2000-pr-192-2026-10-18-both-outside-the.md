# blend.fillet_solver_p1: probe notes (`--fuzz 2000`, PR #192, 2026-10-18; both outside the committed cases)

Both are fuzz "differs" notes (no crash, no panic); the unit's cases and `--perturb` pass.

1. **`trim_rail` after `ReadFromFile(.., rebuild = TRUE)`** (fuzz00617: `Mouse.smb`, edge 7 or 1, `t = [0.286464711202138, 1.0]`, `rebuild = true`).
   The first control point of the trimmed 3D curve and of both UV trim curves differs: `values.flat[8]` cpp `6.134203293918636e-1`,
   rust `6.137686398848857e-1` (rel 5.7e-4); the untrimmed curve (`t = [0, 1]`), `t = [0.2, 0.9]`, edges 0-6, 8, 9 and the same B-rep read without
   `rebuild` all agree bit for bit. Both sides run `SmBSplineCurve::Trim` (unit `nurbs.b_spline_curve_n_lib`) on the same imported curve, so the
   difference is in that trim for this interval, or in per-curve data the record-by-record import does not carry (zone tolerance of a rebuilt
   curve, which the trim uses to remove knots near the cut).
   **Resolved (round 3, 2026-10-19): not an import bug; the owner is this unit's `sm_TrimRail` call into the trim.** The import carries the
   curve and the edge tolerance faithfully: `trim_rail` cases now also compare, before the trim, the 3D curve's unique knots and
   multiplicities and the zone tolerance each of the three trims reads (C++ `SmTol::GetZoneTol3d(curve)`), and those agree on every case
   including this one. The cause is the tolerance: upstream `Trim` reads `SmTol::GetZoneTol3d(this)`, which for a curve owned by an edge
   (the rail's 3D curve) or by an edge use of it (both UV trim curves) is the edge's `GetTolerance()` (`SM_USE_OLDTOL`, `SmTol.cpp`), and
   uses it for the degenerate end-segment test after `sm_CrvTrim` (`N_CrvRemoveDegenSegs`). The Rust `BSplineCurve::trim` documents that
   it serves owner-less curves and reads the system zone tolerance (`1e-5`) from its kernel. The rebuilt `Mouse.smb` edge 7 carries a
   different tolerance (giving the trims the rail's tolerance is the only change that makes the case agree), so at the cut
   `t = 0.2865` the two sides took different branches of the short end-segment test. `trim_rail` now trims with a
   kernel whose zone tolerance is the rail's tolerance (`blend/fillet_solver/p1.rs`, "Rail trims" in the module notes), and the committed
   fixture `trim_rail_mouse_rebuild_edge7` agrees bit for bit. Other callers that trim an edge-owned curve through `Curve::trim` should
   pass such a kernel too (the trim unit's owner-less assumption is documented in `nurbs/b_spline_curve_n_lib.rs`).
2. **`rollover` on `sphere.smb` with a tolerance at the rounding floor** (`tol = +-1e-300`, edges 0 and 4): `|binormal . normal|` is compared
   against `cos(pi/2 - tol) = 6.1e-17`; the C++ result is "tangent", the Rust one is not. Both dot products are below `1e-12` (the
   committed fixture `rollover_sphere_tight`, `tol = 1e-12`, passes), so the flag flips only on rounding noise around zero (presumably the
   sphere's evaluation of the normal or binormal); not investigated further.
