# surface.twist_volume (`SmTwistVolume`)

All difftest cases agree bit for bit with upstream (`max_rel_err = 0`, 1572 default cases,
a 20,072-case run clean); the 3 deferred cases are `ReadFromDB`, whose base-class read
`SmVolume::ReadFromDB` belongs to `surface.volume_p2`. No upstream test builds a twist
volume. Upstream behaviour worth knowing (all ported as written):
- **Constructor repair.** The component constructor compares `Unitize`'s
  `SM_ERR_INVALID_INPUT` with `SM_ERR`, so with a zero axis or mid direction only its last
  branch runs (a frame about the repaired axis `(0, 0, 1)`). A too-small period becomes
  `|dPeriodMinDist|` of the argument, not the clamped minimum (so it can stay below
  `SM_ZONE_TOL_3D`). The component constructor is in no unit's function list; it is ported
  as `TwistVolume::create` because nothing else builds a usable twist.
- **Default construction.** `SmTwistVolume()` has period `0` and minimum `SM_UNDEF_DOUBLE`
  (member initializer); every map then divides by zero (NaN / inf on both sides).
- **`operator==`** overwrites the period comparison with the minimum comparison: twists
  differing only in period compare equal.
- **`EvaluateSimple`'s `Duw`** is `(-per sin a, -per cos a, 0)`; the header documents
  `(-per sin a, per cos a, 0)` and `Evaluate` uses that. Mixed `uw` derivatives of a simple
  twist map are therefore wrong upstream.
- **`EvaluateBoundingBoxSimple`** writes `*pProjBox` unconditionally in the unbounded
  branch (null dereference when only the pseudo box is requested: the port returns
  `InvalidInput`, the ffi rejects it). Its `(long)(theta / 2 PI)` of a NaN quotient is UB;
  the port emulates x86-64 (`LONG_MIN`).
