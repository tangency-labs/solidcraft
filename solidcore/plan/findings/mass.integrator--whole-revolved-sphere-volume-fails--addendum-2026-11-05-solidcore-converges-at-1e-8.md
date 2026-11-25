# Whole revolved sphere: since #2890 the default integrator converges at 1e-8 (2026-11-05)

Updates `mass.integrator--whole-revolved-sphere-volume-fails.md` and its addenda
`addendum-2026-11-01-shared-at-1e-8` and `addendum-2026-11-03-existing-wr472-fix`.

After #2890 (precise properties: the trims' base, the split at the surface's knots, span ends
from their own knot span, the fifth-order step estimate), solidcore's default integrator,
without WR-472's body-length error scale, integrates the whole revolved sphere at 1e-8. The
1e-8 failure is no longer shared; the pinned C++ still returns `SM_ERR` (1001).

| input | accuracy | solidcore (default) | relative error | C++ |
|---|---|---|---|---|
| `sweep.api_brep` wr471_rot_half_circle_whole_sphere_off_3e-7 | 1e-8 | 523.598775627212 | 5.52e-11 | 1001 |
| `sweep.api_brep` wr471_rot_half_circle_whole_sphere_off_0 | 1e-8 | 523.5987756272117 | 5.52e-11 | 1001 |
| `sweep::api_brep::tests::wr471_whole_sphere_volume` | 1e-8 | 523.5987756272146 | 5.52e-11 | - |
| the same test | 1e-10 | `mass::integrator::step_control` 1001 | - | - |

The errors are against 4/3 pi 5^3 = 523.5987755982989. With WR-472's scale the test gives
523.5987756264303 at 1e-8 and 523.5987755985877 at 1e-10.

`wr471_whole_sphere_volume` now expects the default integrator to converge within 1e-8 and
to fail at 1e-10. The two difftest cases are known mismatches
(plan/findings/sweep.api_brep--precise-properties-2890-moves-cases-2026-11-05.md).
