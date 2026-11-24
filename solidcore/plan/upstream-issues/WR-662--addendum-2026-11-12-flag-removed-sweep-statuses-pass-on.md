Updates: WR-662

#### WR-662 (update, 2026-11-12). `sweep_statuses_pass_on` is removed

The sweeps, extrusions and their topology sweep (`SmPrimitiveCreation`, `SmTopologySweep`, the translational and rotational sweep geometry) now always return the statuses SMLib drops and the fork passes on (tangency-labs/smlib c8f6a31, `SER`). Propagation is the only path, and the flag is gone (tangency-labs/solidcraft#14246, batch S5).

Removed:
- the field, and `SweepGeometryCreation::statuses_pass_on`, which `api_brep`'s two non-manifold sweeps copied from it and every other construction of the sweep geometry left off;
- the topology sweep's `pass_on` reader;
- every ignore branch at the sites (`passed_on` / `ignore_or_pass_on` / `ignored_or_pass_on` with the flag);
- `CreateCurveSweep`'s unused path normal, which the fork no longer computes;
- SolidCraft's preset line, its entries in `fixes.json` and in `issue_2260`'s list.

The #2009 group C tests (`issue_2009_group_c_sweep_statuses.rs`, `issue_2009_group_c_sweep_p3.rs`, the planar-path `audit_tests`) keep the passed-on half.

Kept from the old path: `CreateTaperExtrude` still zeroes the caller's normal before `ComputeCrvsNormal` can fail (`SmCurve::ComputeCrvsNormal` sets it to 0 first, and the fork's `SER` returns with it), so `a_curves_normal_that_cannot_be_computed_is_zero` is unchanged.

Tests that pinned a dropped status, re-pinned to the fork's verdict (each cites its `SER` line):
- `sweep::api_brep` `consuming_sweeps_copy_then_run_or_defer`: a capped sweep of a circle and a separate line, whose cap is refused (1001; the curve `CreateLinearSweep`'s `SER( pBrep->CreatePlanarFacesWith3DCurves(..) )`, c8f6a31 `SmPrimitiveCreation.cpp`:1482), where upstream returned the two open walls; and a capped sweep of no curves, refused at the same site, where upstream returned an empty B-rep;
- `tests/wr591_touching_loops_sweep_any_order.rs` `wr591_without_the_flag_the_capped_sweep_refuses` (was `.._returns_an_open_skin`): the same cap site, in the 15 orders whose cap plane is refused without `loops_go_on_round_the_vertex`;
- `sweep::api_brep` `pipe_and_planar_path_validate_then_run_or_defer`: a NaN pipe radius, refused by the profile circle (1001; `CreatePipeSweep`'s `SER( SmCircle::CreateCanonical(..) )`), where upstream returned an empty B-rep;
- `sweep::topology_sweep::engine` `a_failed_collect_is_returned_so_no_unmarked_face_is_retried` (was `ignored_collect_error_cannot_retry_an_unmarked_face_forever`): `SER( sTT.CollectFaces(..) )`;
- `boolean::merge::p3` `planar_boolean_back_edge_reaches_intersector_and_checks_null_output`: Intersection of bare edge-less face records, whose delete fails (`Boolean2D`'s `SER( sTI.DeleteFaces(sDelFaces) )`);
- `primitive::api_primitives` `blend_returns_its_face_or_its_surface_failure` (was `blend_success_can_return_a_face_or_no_face`): a blend whose surface cannot be made (`SmPrimitiveCreation`'s face/edge `CreateBlendPrimitive`, `SER( CreateBlendPrimitive(pEdgeuse1, ..) )`), where upstream dropped the edge-use overload's status and returned success with no face, the box unchanged.

The two real sweeps that failed with the flag gone (`issue_2540`'s extrusion of a spline and two lines, `topology::brep::p9` `issue_15020_reorienting_a_sheet_returns_its_note_with_the_result`) were a port defect, not the fork's verdict: the sweep's split took a line's bare base without its NURB (tangency-labs/solidcraft#15829, fixed by #15940; the taper's twin is #15938). Both pass on main with this change.

- **Results:** goldens blessed on main and with this change, compared row by row: 69 rows move, in 6 units, every one from a result to a status (1001 or 1007). The default kernel now refuses these inputs, as SolidCraft's preset already did. Each row's site was measured (probes s5-why-1, s5-sites-3 to -6, s5-blend-1), and main's result was checked (s5-why-2: `check_brep` Level 2, manifold solid, volume). Fork lines are in c8f6a31's `SmPrimitiveCreation.cpp` unless named.
  - 14 (`primitive_creation_p1` capped sweeps of one open line, arc or skew line at distance −1 or 2, `primitive_9`; `api_brep` `review_linear_empty`): the start cap, `SER( pBrep->CreatePlanarFacesWith3DCurves(..) )` :1482. Main returned a one-face open sheet (Level 2 clean, V 0) where caps were asked for, or nothing. A cap on an open curve cannot exist.
  - 8 (`primitive_creation_p1` arc and skew-line sweeps at distance 0): the extrusion's `SER( pSrfExt->AdjustSTEPUVDomain(sAnalDomain) )` :1404. Main's bodies failed Level 2 (Bad Gap, `SmSurfOfExtrusion` YLength 0) or were empty.
  - 6 (circle sweeps at distance 0, incl. `api_brep` `review_linear_circle_zero`, `_neg_zero`): the cylinder's `SER( pCylinder->AdjustSTEPUVDomain(sExtent) )` :1381. Main: Bad Infinite Region, V 0, or empty.
  - 4 (line sweeps at distance 0): `SER( pBrep->CreateFaceFromSurface(pExtSurface, ..) )` :1436 on the zero-height plane. Main: 0 faces.
  - 10 (`api_surfaces` skin from faces): `CreateSkinFromFaces`' `SER( m_pRegion->GetBrep()->CreateFacesFromSurface(pNewBSS, .., SM_CT_G1, sNewFaces) )` :7158. Main returned success with 0 faces.
  - 6 (`trans_sweep_geometry` intervals of width 0 or 1e-14): `SmTranslationalSweepGeometry::EdgeSweepSame`'s `SER( pNewEdgeCurve->Trim(sIvl) )` (`SmTransSweepGeometry.cpp`:506).
  - 5 (`primitive_creation_p2` pipes): `CreatePipeSweep`'s `SER( SmCircle::CreateCanonical(..) )` :7201. Main built nothing (status 0).
  - 4 (`primitive_creation_p2` tapers, incl. `ub50_taper_line_and_its_reverse`, `ub50_taper_two_identical_lines`): `CreateTaperExtrude`'s `SER( SmCurve::ComputeCrvsNormal(..) )` :3871 (1) and :3883 (3). Main built nothing.
  - 2 (`primitive_creation_p2` gen0027, gen0046: a partial sphere of radius 2e-6 and a partial cone of height 5e-12): `CreateRotationalSweep`'s `SER( sFrame.SetCanonical( sBasePoint, sLineVector, sYAxis ) )` :2856, the frame of a linear profile whose tangent degenerates (`Vec3::unitize` "zero-length vector"; measured by a backtrace at the error, probe s5-bt-1). Main built nothing; its later zero unitizes in `order_curves_into_loops` were swallowed.
  - 5 (`api_primitives` blends): `SER( CreateBlendPrimitive(pEdgeuse1, ..) )` :7882, after `create_skinned_surface` ("the boundary continuity cannot be met"). Main returned success with the box unchanged and no blend face.
  - 3 (`api_primitives` cylindrical boxes of length 1e-12, length 1e18 and outside radius 1e18): `CreateCylindricalBox`'s cylinder faces, `SER( pBrep->CreateFaceFromSurface(pCylinder, ..) )` :620 / :628 (length 1e-12), and end faces, `SER( pBrep->MakeFaceWithCurves(..) )` :544 / :581 (1e18). Main's bodies were not manifold, or failed Level 2.
  - 2 (`api_brep` drafts): at 90°, `SER( pComposite->CreateTrimmedOffsets(..) )` :3987; at height 0, `SER( pBrep->CreateFaceFromSurface(pRuledSurface, ..) )` :4141. Main: an open face, or a sheet with Bad Infinite Region, V 0.
- **Follow-up, not in this change:** adjacent-edge blends on one face (box edges 6+7 on face 4, 10+11 on face 5, and the generated gen0201/gen0288) fail in `create_skinned_surface` ("the boundary continuity cannot be met") at any scale; main hid it as success without a face. The finding `primitive.api_primitives--native-blend-primitive-coverage-pr-831-2026-10-25` (blocker 2) measured C++ building faces for nearby pairs; these exact pairs are not C++-measured.
