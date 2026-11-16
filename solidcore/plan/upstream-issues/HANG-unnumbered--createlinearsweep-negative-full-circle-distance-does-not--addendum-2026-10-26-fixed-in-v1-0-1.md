Updates: HANG-unnumbered (plan/upstream-issues/HANG-unnumbered--createlinearsweep-negative-full-circle-distance-does-not.md)

#### HANG-unnumbered (update, 2026-10-26). Fixed in v1.0.1

Fixed in v1.0.1 (3fd5b95): SmPrimitiveCreation::CreateLinearSweep (curve overload), a negative dSweepDist is negated together with the unit sweep direction before the analytic domain is built, so the full-circle SmCylinder branch bounds a forward V domain. Verification: primitive.primitive_creation_p1 review cases linear_circle_cap{0,1}_distance-1 and the analytic-circle witnesses, no longer rejected by the shim, pending the pin101 difftest run.
