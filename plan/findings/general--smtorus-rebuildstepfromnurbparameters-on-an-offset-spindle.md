# `SmTorus::RebuildSTEPFromNURBParameters` on an offset spindle torus (surface.torus, 2026-10-22)

Found while widening the `surface.torus` generator (the widened draw was not kept; the
committed generator leaves the existing cases unchanged): op `offset` with `rebuild`,
torus major 3.2965, minor 4.2536 (minor > major), swap, inside-out, explicit generator
circle, domain [169.23, 0, 310.95, 360], dist 2.9284, tol 1e-6. C++ returns `SM_ERR`
(1001) for the rebuild; Rust returns `InvalidInput` ("iso circle output not written",
recorded as -1). Upstream reads `sm_TestForIsoCircles` outputs that the call did not
write (`dRadiusU/V` are uninitialised doubles), which is undefined behaviour, so the C++
value is not a reference. Not fixed; the shim does not guard it.
