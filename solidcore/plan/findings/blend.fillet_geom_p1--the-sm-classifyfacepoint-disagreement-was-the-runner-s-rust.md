# blend.fillet_geom_p1: the `sm_ClassifyFacePoint` disagreement was the runner's Rust face, not the kernel (2026-10-22)

Follow-up to "blend.fillet_geom_p1: `sm_ClassifyFacePoint` returns 0 where the C++ returns
1 or 2" above (the cause of PR #268's 5 `blend.fillet_corner_p2` failures).

Cause: the shim builds every face with `face_of` = `SmBrep::CreateFaceFromSurface` (an outer
loop of four natural trim edges, edge and face tolerances set), but the runner's Rust side
(`classify_face_point`, the model's target faces, `evaluate_mate`) added a bare face with no
loops and tolerance 0 to a `Brep::new()`. In `sm_ClassifyFacePoint`,
`SmTol::GetZoneTol3d(pOrigFace)` is the face's tolerance (old-tolerance build:
`pTopo->GetTolerance()`), so the Rust `Point3DClassify` ran with a zero tolerance on loopless
faces and returned `SM_PC_UNKNOWN` for both faces (result 0), where the C++ classified the
point into the face it lies in. `classify_face_point` itself matches upstream line by line
(branch order, `ContainsPoint2d` default tolerance 0, guess UVs, flags).

Fix (runner only, no kernel or shim change): the Rust side builds the faces as the shim does
(`create_face_from_surface` on a `new_brep`). Those faces have no UV trim curves yet, and
`CalculateUVDomainFromUVTrimCurves` makes them with `SmSurface::DropCurve`
(`intersect.surface_drop_lib_p3`, todo), so these cases now defer instead of comparing two
different setups: `blend.fillet_geom_p1` 471 pass -> 457 pass + 20 deferred (14 previously
compared, the 6 new corner-model fixtures), `blend.fillet_geom_p5` 300 pass -> 274 pass + 26
deferred (`split_face`); both green -> partial. On main merged with #268 (plus #268's own
pending adaptations), `blend.fillet_corner_p2`'s `gen0014` and
`corner_update_split_face_08/_11/_71/_75` defer instead of failing (0 failed).

Considered and not done: also attaching the natural UV trim curves in the shim's `face_of`
(`CreateUVTrimCurves` with the natural curves) so the cases get past `DropCurve`. They then
stop at `Point3DClassify`'s trimmed surface cache face handle (`UNPORTED(topology.face_p2)`)
instead, and every face-building op (`evaluate_mate`, every `fb` op) enters HANG-29's
`CreateUVTrimCurves` regime: the FFI fuzz sweep hung 30 s in `EvaluateMate` on a control
point of `-DBL_MAX`, which would need the `MAX_MAGNITUDE` bound on those ops too.
