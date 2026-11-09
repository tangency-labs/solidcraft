Updates: WR-28

#### WR-28 (update, 2026-10-31). Fixed in v1.0.1

Fixed in `usd-brep` v1.0.1 (`3fd5b95`): `SmProjectedCurve::MakeOrthoNormal` (`source/SMLib/src/SmProjectedCurve.cpp:220, 229`) negates `bProjZero` and `bAuxZero` (`if(bProjInit && !bProjZero)` and `if(bAuxInit && !bAuxZero)`), unitizing non-zero projection vectors (and orthonormalizing `m_sAuxData` for `SM_PT_ROTATION`) while returning `SM_ERR` for uninitialised or zero vectors. `ProjectedCurve::make_ortho_normal` (`crates/solidcore/src/curve/projected_curve.rs`) already ports the v1.0.1 logic faithfully; pinned by unit tests `wr28_parallel_non_unit_proj_vec_evaluates_without_skew`, `wr28_zero_and_uninitialized_proj_vec_fail_with_sm_err`, and `wr28_rotation_orthonormalizes_and_rejects_degenerate_aux_data` (#2139).
