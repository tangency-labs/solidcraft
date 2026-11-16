# curve.conic

Green, no disagreement. Upstream behaviour worth knowing:

- **`SmConic::Evaluate` zero-tangent fix reads an unwritten slot.** With
  `bNonZeroTangents` and two derivatives, the fix of a tiny first derivative reads
  `aPointAndDerivatives[2]` *before* the second derivative is written there (it is written
  afterwards), i.e. whatever the caller's array held. For a fresh `SmVector3d` array
  (`SM_UNDEF_DOUBLE` members, length ~2e-61) the fix never fires; the port reads
  `Vec3::UNDEF` there, so it matches every caller that passes a fresh array. The
  `lNumDerivatives < 2` recursion branch is unreachable (the function returns after one
  derivative before the fix), and `lNumDerivatives > 2` fills three vectors and returns
  `SM_ERR`.
- `SmConic::IntersectImplicitConic` (and so `IntersectConic`) of an `SM_CT_UNKNOWN` conic
  solves a quartic of uninitialised coefficients (UB); the port returns `InvalidInput`,
  the shim rejects such inputs. `EvaluatePoint` of an unknown conic leaves `x`, `y` of the
  caller's point untouched (port: a fresh point, `SM_UNDEF_DOUBLE`).
- `IntersectConic` ignores both intervals. The upstream test asserts on solution counts
  in `smCurveTest.cpp` (lines 6053-6499) all hold in both implementations.
- `SmConic::ReadFromDB`: an ASCII type code whose extraction is skipped leaves
  `lConicType` uninitialised (port: `InvalidInput`); type codes outside `0..=3` behave as
  `SM_CT_UNKNOWN`.
