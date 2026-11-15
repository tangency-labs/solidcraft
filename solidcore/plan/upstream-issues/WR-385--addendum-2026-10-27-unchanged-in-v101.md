Updates: WR-385

Source rechecked against the official NVIDIA usd-brep 1.0.1 pin,
`3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5`.
The complete `SmCone::RebuildSTEPFromNURBParameters` function in
`source/SMLib/src/SmCone.cpp` is identical to official 1.0.0 at `c9c5979`.
It still reconciles radii only within `SM_EFF_ZERO * (1 + r)` and assigns a
semi-angle from larger differences, including on an `SmCylinder`.
The cylinder-specific reconciliation quoted in #713 belongs to the
`tangency-labs/smlib` fork (`f06b48a` as cited there), not this official release.
Thus adoption of the project's 1.0.1 pin does not remove the recorded defect.

- **Confirmed on official 1.0.1:** yes, the `issue713_*` native differential
  fixtures in `surface.cone_p2` compare the unpatched C++ state, including its
  nonzero semi-angle for the reported rounded radii. All 1,596 cases pass at
  zero relative error with the fix disabled; enabled-path tests independently
  require zero angle or reject a taper before changing analytic state.
