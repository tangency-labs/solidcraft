Updates: WR-385

PR: https://github.com/tangency-labs/solidcore/pull/724

The original PR recorded this defect under WR-257 in the former shared archive.
Main independently recorded the same defect as
[WR-385](WR-385--cylinder-rebuild-gets-semi-angle-from-rounded-radii.md) and added
`Kernel::fixes.cylinder_rebuild_mean_radius`. This PR now uses that existing ID and
flag; no second issue ID or fix flag is introduced.

- **Location:** `source/SMLib/src/SmCone.cpp:4695`,
  `SmCone::RebuildSTEPFromNURBParameters`, end-radius equality and semi-angle update.
- **Trigger:** radius `9.0000000000985`, height 3, full angular domain; scale X/Y
  of the last NURB generator row by `9.0000000000757 / 9.0000000000985`.
- **Confirmed:** yes, an unpatched prebuilt `c9c5979` C++ probe and the
  `surface.cone_p2/issue713_*` differential fixtures return success (1000), radius
  `9.0000000000985008`, and semi-angle `-4.3550699142343645e-10` degrees in both
  UV orders. Larger tapers also succeed while keeping the object's cylinder type.
- **Compatibility:** the default remains the pinned SMLib behavior. The correction
  is opt-in through the existing flag; it requires neither an SMLib 1.0.1 reference
  upgrade nor a C++ oracle patch. The fork fix `tangency-labs/smlib@f06b48a` is the
  related upstream change referenced by #713, not a runtime/build dependency.
- **Additional regression:** a small-arc cylinder can already carry a nonzero
  semi-angle from an earlier rebuild. Equalizing the recovered radii alone leaves
  that angle unchanged: upstream updates it only when their difference exceeds
  `SM_EFF_ZERO`. The enabled fix explicitly clears it after accepting the radii;
  rejection still precedes every analytic-state change.
