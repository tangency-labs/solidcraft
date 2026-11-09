# blend.fillet_executive_p3: SmApiFilletEdges on corner_box.smb / nurb_box.smb disagrees (2026-10-26)

Unit: blend.fillet_executive_p3 (end-to-end runner, `real_solid` with a `file` source).
PR: tangency-labs/solidcore#1101.

Inputs: `SmApiFilletEdges(r = 0.1, xsect 1, G1, thumbweight 1)` on
`pt_TestFiles/Fillet/corner_box.smb` (every edge; edge 9; edge 13) and
`pt_TestFiles/Fillet/nurb_box.smb` (every edge; edges 0 and 5). The C++ reads the file with
`SmBrep::ReadFromFile`; the Rust side imports the `topology.brep_p1` export of the same
read (`use_arena_naming` afterwards). Other single edges of corner_box (0, 4, 8, 12) pass
at 1e-9 with real fillets.

Both values (first differing key `mass_properties[0]`, the area):

| case | C++ status / area | Rust status / area |
|---|---|---|
| corner_box every edge | 0 / 360.3022361392337 | 1001 / 364.2351773164176 (body unchanged) |
| corner_box edge 13 | 0 / 364.2382213623975 | 1001 / 364.2351773164176 (body unchanged) |
| corner_box edge 9 | 0 / 364.0502109116303 | 0 / 364.05026992193945 (fillet made, values differ) |
| nurb_box every edge | 0 / 565.7468284408683 | 1001 / 572.9129579786076 (body unchanged) |
| nurb_box edge 0 | 0 / 572.4673922434714 | 1001 / 572.9129579786076 (body unchanged) |

Suspected causes (localised with temporary prints, not fixed here):

1. nurb_box (and corner_box edges on its SmSurfOfExtrusion faces): `CalcFilletGeom`
   returns SM_ERR_NULL_POINTER from `blend::fillet_solver::p4::c3_bspline_base`.
   Upstream (SmFilletSolver.cpp:1960-1977) uses `SM_CAST_PTR(SmBSplineSurface, ..)`, an
   `IsKindOf` test; `SmSurfOfExtrusion`, `SmCone`/`SmCylinder`, `SmSphere` and `SmTorus`
   all derive from `SmBSplineSurface` (SmSurfOfExtrusion.h:83, SmCone.h:123,
   SmSphere.h:91, SmTorus.h:92), but `is_bspline_surface` in that file accepts only
   BSpline, Plane and SurfOfRevolution. The base surface here is type 15030
   (SmSurfOfExtrusion). Widening the test (tried locally) moves the failure on: the run
   then fails in `CalcCornerEdgeGeom` -> `FilletBrep::calc_fillet_int_side_face`
   (`blend/fillet_geom/p5.rs`, the `fillet_surface()` of the primary edgeuse's fillet
   geometry is `None`), so the widening alone does not make the case agree and was not
   committed.
2. corner_box edges 9/13 (both base surfaces planes, the analytic cylinder fillet
   succeeds): a later stage differs; edge 13 fails with 1001 inside
   `calculate_corner_edges`, edge 9 completes with an area 1.6e-7 relative off. Not
   localised further.

The cases are listed in `crates/difftest/known_mismatches/blend.fillet_executive_p3.toml`.
