# heal.face_props (`SmFaceProps` / `SmLoopProps` / `SmLooptreeItem`, 2026-10-16; partial, no disagreements)

- **UB-131 (`plan/upstream-issues.md`).** `SmFaceProps::SetProps_Gaps` returns at once from stage
  `GAPSa` on (`SmFaceProps.cpp:768`), so the `m_pFace != cpFace` re-run in `SetProps_Stage2`
  (`:875`) and `SetProps_Stage3` (`:1258`) never sets `m_pFace`; a stage past the gaps with no
  face (`ReSet(2a)` then `SetProps(2)`, or a seeded stage `GAPS` then Stage 2) dereferences NULL
  (`:898`) and the reference segfaults. No disagreement: the harness rejects those scripts
  (`ffi::validate` simulates the stage and the face pointer) and the port returns `InvalidInput`.
  Regression fixtures: `rejected_reset_then_stage2`, `rejected_seeded_gaps_then_stage2`.
- **UB-128 (`plan/upstream-issues.md`).** `SetProps_Stage3` on a plane with a degenerate domain
  (`[0, 0, t, t]`) and edge loops crashes in `SmFace::ClassifyLoops` (`SmCrvOnSurf::GetKnots`,
  `SmCurveClassification::IntersectUVDiscontinuties`); the crash is not monotone in `t`
  (crashes at 1e-300, 1e-100, 1e-14, 1e-12, 1e-10; survives at 1e-11, 1e-9). No disagreement:
  `ffi::validate` rejects a script that can reach Stage 3 on such a face when a domain side is
  below 1e-6. Regression fixtures: `rejected_degenerate_plane_stage3_*`.
- **Why `UNPORTED(topology.face_p2)` stays in `surface_approx_tol_3d` (`surface/surface/p3.rs`).**
  A `Surface` holds only an owner handle, not its B-rep, so `SmFace::GetTolerance` of the owner
  cannot be called from a bare surface. The face props have the face and its tolerance, so they
  pass it (`Surface::get_singularities_of_face`, `face_tolerance = Some(..)`); every other caller
  still reaches the stub. Wiring it needs the surface to reach its B-rep (or every caller to pass
  the tolerance), which is a data-model change for `topology.face_p2`'s owner, not for this unit.
- **Coverage limits.** Stage 3 is compared through op 8 (`SmLoopProps::SetProps` of each loop);
  every direct `SetProps_Stage3` case defers at `SmFace::ClassifyLoops` (`query.face_p4`), so
  the Stage 3 loop counts and the containment are not compared until that unit lands.
- **`m_lBadFlatCorners` is compared but never nonzero.** The snapshot holds the flag value and
  the four flat corner points (an unset corner is the uninitialised point on both sides), so a
  nonzero value would be compared value for value. No generated or fixture face reaches one: a
  corner is flat only where `Su` and `Sv` are parallel and neither is degenerate, which the
  plane (axes at least 1e-3 apart) and the canonical surfaces of revolution the harness builds
  never give (a trial with a generator along the circle's tangent at its start still reported
  none on either side, and was dropped). Only the seeded field (op 5, field 9) exercises the
  value, so the computation in `FaceProps::set_props_stage2` (`GetFlatCorners`) is compared
  as "none". `SmSurface::GetFlatCorners` uses `lRtn &= SM_FC_*` (not `|=`) when it fills the
  points array (WR-55), which drops other corners' flags; the port follows
  the upstream expression.
- **Harness.** The shim and the runner build the same one-face B-rep: a plane or an
  `SmSurfOfRevolution::CreateCanonical` surface (cylinder, cone with or without apex, sphere,
  torus; `AdjustSTEPUVDomain` opens the 360 degree sweep) owned by the face, with a vertex loop
  or edge loops (outer loop and holes of `SmLine` edges). The Rust B-rep is rebuilt record by
  record (`rust_brep`) and the shim checks the realised edge use order. The face tolerance reaches
  `GetSingularities` through `Surface::get_singularities_of_face` (a `Surface` holds only an owner
  handle; the other `UNPORTED(topology.face_p2)` tolerance stubs on surfaces are unchanged).
- **Deferred by design.** Closed faces (any full sweep): `SetProps_Stage2` reaches
  `SmFace::HasSeamProblem` (`topology.face_p4`); every `SetProps_Stage3` reaches
  `SmFace::ClassifyLoops` (`query.face_p4`). Op 8 compares `SmLoopProps::SetProps` of each loop
  (the loop-props loop of Stage 3) without the classification. Wire loops cannot be built with
  `SmBrepConstructor` (it closes every loop) and are not compared.
