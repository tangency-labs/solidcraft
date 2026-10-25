# curve.hermite_curve (partial; no disagreements)

2131 difftest cases (131 fixtures + 2000 generated): 2038 bit-identical, 93 deferred
(`IsKindOf` of a type other than `SmHermiteCurve_TYPE` reaches `SmCurve::IsKindOf`,
unit `curve.curve_p2`). Upstream behaviour worth knowing:

- **`Evaluate` leaves its last entry unset for `lNumDerivatives >= 4`.** The zeroing
  loop is `for (i = 4; i < lNumDerivatives; i++)`, so `aPointAndDerivatives[n]` keeps
  whatever the caller had (an `SmVector3d` default is `SM_UNDEF_DOUBLE`). The port
  returns zero there; the shim pre-fills the caller array with zeros so the two agree.
- `ReadFromDB` reads into `new SmHermiteCurve()` whose vectors are `SM_UNDEF_DOUBLE`
  (the `SmVector3d` member initialisers), so a skipped ASCII extraction (stream
  already failed) leaves `1.23456e-61`, and a value that fails to parse becomes `0`.
  Fixtures `db_ascii_truncated`, `db_ascii_garbage`, `db_ascii_empty` cover this. A
  short *binary* read is undefined upstream (see `topology.database_io`) and not tested.
- `CalculateBoundingBox` sets the basis of the caller's pseudo box without resetting
  its intervals (unless the chord is zero, which resets the whole box), so a pseudo
  box that already holds points keeps them; fixtures `*_preset` cover it.
