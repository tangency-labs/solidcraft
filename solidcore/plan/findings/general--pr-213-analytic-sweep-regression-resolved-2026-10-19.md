# PR #213 analytic sweep regression resolved (2026-10-19)

Real analytic skew lines from (0,0,0) to (1,0,1), swept along Z at distances
0/2/-1 with either cap flag, exposed Rust status 1001 from the raw B-spline
base versus native success before the missing face dependency. The extrusion
now materializes a lazy line through its virtual MakeNurb and applies existing
owner parameterization. A finite swapped extrusion with domain [-.5,-2,2,3]
also exposed a classification point of (0,0,0) versus native (-.5,0,-.5): native
GetEnds reenters owner construction while IsLinear retains its earlier sample
domain. The shared classification suffix now accepts that captured domain and
the constructor preserves the same side effects and operation order.

Six direct surface regressions compare complete native state; extrusion p1
passes 2061/2061 exactly. Primitive p1: 773 cases, 689 exact passes, zero failures,
75 dependency deferrals and nine native rejections; all 689 perturbations are
detected. Curve p5: 3095 passes/two deferred; extrusion p2: 1148 passes/225 deferred;
translational sweep geometry: 719 passes/50 deferred/19 rejected, all zero failures.
No tolerance or absolute floor changed. Kernel tests: 1398 passed. The complete
local --green gate: 213 units, 114 passed, 99 partial, zero failed. Both focused
500-case FFI fuzz runs completed without differences, crashes, panics or timeouts.
