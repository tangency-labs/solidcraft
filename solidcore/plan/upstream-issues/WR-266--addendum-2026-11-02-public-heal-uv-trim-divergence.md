Updates: WR-266 (and WR-268)

#### WR-266 (update, 2026-11-02). The opt-in flag's public effect is a UV trim loss, not the retry marker
- **What changed:** the reachability of `Kernel::fixes.heal_stage3_failures_propagate` (both
  records' single field) is now measured through the public entry point instead of the
  `FaceProps` unit. On a public body it does change a healed body's observable output, but the
  effect comes from WR-268's classification-status half, not from WR-266's marker reset.
- **Body and operations (public APIs only):** `primitive::api_primitives::create_cylinder(k,
  Point3::new(0.0, 0.0, 0.0), 1.0, 1e-9)` — a flat cylinder whose heal makes a new side face
  that `SmFace::ClassifyLoops` is handed with no loops — then
  `heal::heal_data::p1::api_heal_brep(k, Some(&mut b))` (the whole sequence, no UV trim pass),
  with every other `Kernel::fixes` field at its `Default`.
- **Stage-3 failure reached mid-heal:** `Cache_FaceProps_Stage3` calls `SetProps_Stage3` on
  that new face and it fails with `m_ePropStage == SM_PROPSTAGE_3a`. Default kernel: the
  `ClassifyLoops` status `SM_ERR` (1001, `query::face::p4::classify_loops`) is dropped, the empty
  classification is read, and the missing loop tree root reports `SM_ERR_NOTYET_HEAL_FACE`
  (1040, `heal::face_props::FaceProps::set_props_stage3`); `Cache_FaceProps_Stage3` reacts to
  1040 by moving the face to the not-yet list, so `tgt_face_props` no longer contains it. Flag
  on: the classification status 1001 is returned before its output arrays are read (WR-268), the
  heal's status policy ignores it like any other `SmStatus`, and the face stays a target.
- **Measured difference.** Both modes return `Ok(())` from `api_heal_brep` and produce the same
  body apart from UV trim curves: `shells=3 faces=3 loops=3 edges=2 vertices=1 regions=3
  faceuses=6 loopuses=6 edgeuses=8`, three faces and two edges at tolerance `1e-5`, one vertex at
  `1e-5`, three faces with a surface, region void flags `[1, 0, 0]`. In edge-use arena order the
  edge uses with a UV trim curve are `[true, false, false, true, true, false, true, false]` by
  default and `[false, false, false, false, false, false, false, false]` with the flag on: the
  face that stays a target reaches `Fix_SplitFaceAtSeams`, and the body's UV trim curves are
  observed gone after it (a `through = CacheFaceprops3` run is identical in both modes, only the
  `through = FixSplitfaceAtseams` run differs; the clearing call site is not traced further
  here). The heal path carries no attribute state (`heal.heal_data_p3`; source reading, not
  asserted by the test), so no attribute is reported or changed in either mode.
- **WR-266's half is not publicly observable in one heal.** `Cache_FaceProps_Stage3` runs once per
  `HealBrep` and the `HealData` (with its `FaceProps` markers) is discarded when the entry point
  returns, so the Stage2-versus-Stage3A marker is never read again. Disabling only the marker reset
  keeps the measured difference; disabling only the classification-status propagation removes it.
  The retry difference stays pinned by the existing `heal.face_props` unit tests
  (`stage3_failure_keeps_in_progress_marker_by_default`,
  `issue_521_stage3_failure_does_not_certify_cached_properties`).
- **Test:** `crates/solidcore/tests/wr266_public_heal_stage3_failure_uv_trims.rs` pins both modes'
  exact status and body. Forcing the classification-status propagation off makes its flag-on half
  fail; forcing it on makes its default half fail; the kernel is unchanged.
- **Limits:** a Rust-only measurement; no C++ run was made for this body (its default result is
  the SMLib behaviour WR-266/WR-268 describe, not a new native comparison). Thirteen other public
  bodies — box, `1e-9`/`1e-6`/`1e-8` thin boxes, cylinder, cone, sphere, sphere without poles,
  torus, pyramid, boolean union, boolean difference and a rotational sweep, each with and without
  the UV trim pass — showed no difference in status or body. A separate probe added a
  dangling-loop-use edit to the first face of the box, thin-box, cylinder, cone, sphere,
  sphere-without-poles, torus and pyramid bodies and also healed a public one-face sheet body:
  none produced a stage-3 failure, and every one of them healed identically in both modes.
  The only other mid-heal stage-3 failure found is the `1e-9`-high box (`faces 2..5`,
  four times per run) with status 1040 alone; both modes quarantine those faces, so its healed
  body is identical too.
