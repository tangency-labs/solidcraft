Updates: HANG-13

#### HANG-13 (update, 2026-10-26). Fixed in v1.0.1

Fixed in v1.0.1 (3fd5b95): SmPrimitiveCreation::CreateLinearSweep (curve overload), the sweep vector is unitized and a negative distance becomes a positive one along the reversed direction before the analytic domain SmExtent2d(0, 0, 1, dSweepDist) is built, so the full-circle cylinder branch no longer gets a reversed V domain. Verification: fixtures/sweep.api_brep/hang13-negative-full-circle.json and the two primitive.primitive_creation_p1 cases linear_circle_cap0_distance-1 and linear_circle_cap1_distance-1, now compared with the FFI and shim preflights removed, pending the pin101 difftest run.
