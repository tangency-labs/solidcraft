# curve.projected_curve (`SmProjectedCurve`)

Difftest: 2727 cases (227 fixtures re-expressing the two upstream constructions in
`SmTransSweepGeometry.cpp:794` / `SmRotationalSweepGeometry.cpp:586`, perspective views,
`MakeOrthoNormal` edge cases and `WriteToDB`-format streams, plus 2500 generated), all
bit-identical. The shim reads the private members through the explicit-instantiation access
rule (no upstream change). Upstream behaviour kept as is:

- **`MakeOrthoNormal` is inverted.** It unitizes `m_sProjVec` only when it `IsZero()`
  (giving `(0, 0, 1)`) and returns `SM_ERR` for every non-zero vector, which is left
  un-normalised; for `SM_PT_ROTATION` the same holds for `m_sAuxData`, and the
  `NormalInPlane` result is discarded. Callers (the sweep geometry above) pass vectors that
  are already unit, so the projections still work; a non-unit view normal gives a skewed
  "projection" (both sides agree). The constructor ignores the status; `SetCanonical`
  returns it.
- **`Transform` needs `m_bOwnsCurve == UNSURE`.** It tests `m_bOwnsCurve < 2`, so a curve
  that owns its base (`TRUE`, e.g. every curve from `ReadFromDB`) or borrows it (`FALSE`)
  fails with `SM_ERR` (after a pre-edit notify); only the member-initialiser value
  `UNSURE` passes. `EditParameterization`, `ReverseParameterization` and `Trim` refuse only
  `FALSE`.
- **`Copy` of a curve owning a Hermite base** (`SmCurve::Copy` = `SM_ERR`, ignored by the
  copy constructor) returns `SM_SUCCESS` and a projected curve with a null base curve that
  crashes on use. The port returns the `SM_ERR`; the shim maps upstream's null-base result
  to `SM_ERR`.
- `EvaluatePoint` computes the parallel projection before the switch and again inside it;
  `Evaluate` projects perspective derivatives about the already projected position.
- `ReadFromDB` casts the stored ULONG to `SmProjectionType` (undefined beyond `0..=3`; the
  port maps it to `Unknown`) and does not call `MakeOrthoNormal`.
