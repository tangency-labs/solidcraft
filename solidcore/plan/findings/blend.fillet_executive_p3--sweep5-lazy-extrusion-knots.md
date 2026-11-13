# blend.fillet_executive_p3: sweep5 lazy surface-of-extrusion knots refuse a fillet (2026-10-27)

Unit: `blend.fillet_executive_p3`. Fixture: `fixtures/blend.fillet_executive_p3/sweep_round5.json`.

Inputs: file sources `Fillet/letter_g.smb` edge 1 and `Fillet/bottle.smb` edge 29, circular
G1 round, radius 1.0, thumbweight 1 (`sweep5-letter_g-e1-r1.0`, `sweep5-bottle-e29-r1.0`).
The probe sweep (every `Fillet/*.smb`, single edges 0..29 at r=1.0) found the same failure
on `letter_g` edges 1, 2, 3, 5, 13, 15, 16, 17 and `ext_w_hole` edges 4 and 14.

Values:

| case | C++ status / V,E,F / mass | Rust status / V,E,F / mass |
|---|---|---|
| letter_g e1 | 0 / 22,33,13 / ok | 1001 / 23,40,14 / 1007 |
| bottle e29 | 0 / 47,62,20 / 1007 | 1001 / 30,46,18 / ok |

First divergence. The fillet face meets a surface of extrusion made lazily
(`IsNurbSurfaceSurfOfExtrusion`: no NURB until `GetOrCreateGwNurbPointer`). Upstream
`SmSurface::GetKnots` is virtual and makes that NURB. Rust has kernel-free
`Surface::knots` calls that read the NURB-less base and fail with `SM_ERR` from
`BSplineSurface::knots`:

- letter_g e1: `SmFilletExecutive` stitching to the original B-rep. `SmStitchCallback::SelectTopologyToKeep`
  calls `SmCurve::SurfaceMaxDistanceBetween`, which calls `crSurface.GetKnots(SM_SP_U/V)` for the
  sample count. In Rust (`curve/curve/p2.rs` `surface_knot_count`, through
  `SurfaceEvaluate::knots`) the second drop, curve to the extrusion face, fails. Then
  `select_topology_to_keep` has no distance (its documented "uninitialised local"
  branch) and returns `SM_ERR`. All five stitch tries fail. C++ stitches 5 edges on the first try
  (status 1000, lamina 0).
- bottle e29: `IntersectInsertRelate` -> `SmSurface::DropCurve` -> `SmSurfaceDropCurve`, at
  `SnapToKnots` and `GetDomainWithContinuity` (`Surface::snap_to_knots`,
  `Surface::domain_with_continuity_using`). Both read the knots without making the NURB.

Suspected cause: the "Lazy NURB degree and knots" deviation in `surface/surface/p2.rs`
lists these kernel-free callers. Fix: route them through `Surface::knots_with_nurb`.
