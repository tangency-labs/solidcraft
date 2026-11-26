# curve.crv_in_volume (`SmCrvInVolume.cpp`)

No Rust-vs-C++ disagreement: 1426 of 1426 non-deferred difftest cases bit-identical (30
fixture cases built around the `SmSrfInVolume.cpp:1219` usage `SmCrvInVolume(*pBSC,
bInParamSpace, vol, 0, 1)` and the `smCurveTest.cpp` line, plus 2000 generated);
`--perturb` is SENSITIVE. 604 cases are deferred: everything that reaches the volume
(`SmVolume`, `surface.volume_p1` 603, `surface.volume_p2` 1) — evaluation, uncached
breaks (`GetKnots` starts with `GetNaturalParamDomain`), continuities at interior breaks,
equality of equal curves, non-identity transforms, the volume part of `ReadFromDB`.

The port adds `CurveKind::CrvInVolume` and a stand-in `curve::crv_in_volume::Volume`
(type code only; every volume call is `Unported`). **Whoever ports `surface.volume_p1`
should replace that struct with the real volume type** and wire the `UNPORTED(surface.volume_p1)`
/ `(surface.volume_p2)` stubs in `curve/crv_in_volume.rs` (volume calls and the
`smvol_Lift*Derivative` lifts); the `SmCurveClassification` stub there points at
`intersect.curve_class_p4` / `_p8` / `_p1`. `Curve::approx_eq` now returns `Result<bool>`
(the volume comparison can be unported). The C++ side uses an identity `SmTransform` as
the volume and loads the protected break cache through a derived test class.

Upstream behaviour worth knowing:

- `GenerateMults` indexes `m_sConts` by the *knot* index `ii`, not the matched break index
  `bi`; with an interval, `GetKnots`' cached path therefore reports multiplicities of the
  wrong breaks. Its state check returns before sizing the output, so `GetKnots` (which
  ignores the status) then returns no multiplicities; `GetNumberNaturalKnots` then reads
  past the empty array (UB; the ffi rejects that shape).
- `GetKnots`' cached path and computed path give different multiplicities for the same
  break (the computed path keeps the curve's end multiplicity, e.g. 4; the cached path
  derives 3 from `SM_CT_DISCONTINUOUS`). The `&self` dispatch through `Curve` does not
  store the cache (no `const_cast`), so it always answers like upstream's first call.
- `EvaluatePoint` maps the point with `MapPoint` unconditionally before choosing
  `EvaluatePoint` / `MapPoint`; `EvaluateVolume` (not this unit) swaps the two branches
  relative to `Evaluate`.
- The copy constructor copies the volume when it owns the curve *or* the volume; with owner
  flag 1 the copied volume is not owned and leaks. Copying an owned `SmHermiteCurve` base
  fails silently, leaving both objects owning one curve (double delete).
- `CalculateContinuities` with no breaks reads `SetAt(0)` of an empty array and loops on
  `size - 1` underflow (UB); `ReadFromDB` of `SmVolume_TYPE` dereferences a null volume.
