Updates: WR-385

`Kernel::fixes.cylinder_rebuild_mean_radius` now follows the fork's code exactly
(tangency-labs/smlib@5df5b27, a cherry-pick of f06b48a, `SmCone::RebuildSTEPFromNURBParameters`
in `source/SMLib/src/SmCone.cpp`). The flag keeps its name.

The fork's order:

```cpp
if (smos_Fabs(dBotRad-dTopRad) < SM_EFF_ZERO*(1.0+dBotRad))
  {
    dTopRad = dBotRad;
  }
else if (IsKindOf(SmCylinder_TYPE))
  {
    if (!(smos_Fabs(dBotRad-dTopRad) < SM_EFF_ZERO_SQRT*(1.0+dBotRad)))
      { SER_MSG(SM_ERR, _T("Modified Nurb SmCylinder: Top and Bot radii differ")) ; }
    dBotRad = dTopRad = 0.5 * (dBotRad + dTopRad);
  }
```

The port tested the cylinder rule first. Radii within `SM_EFF_ZERO * (1 + r)` therefore got
their mean, where the fork (and upstream) copies the bottom radius. Probe batch 53i (a cylinder
of radius 5.123456789 turned 0.1 rad about (1, 2, 3) and moved to (1234.5, -987.25, 333.3))
found 59 of 64 probed radii 1 to about 80 ulps from the fork's rule.
The flag's body now copies the bottom radius within `SM_EFF_ZERO * (1 + r)` and takes the mean
only beyond that, up to `SM_EFF_ZERO_SQRT * (1 + r)`, as the fork does. Further apart is still
refused (`SM_ERR`) before any analytic state changes.

One line is not in the fork's hunk: with the flag on, an `SmCylinder`'s `semi_angle_deg` is set
to 0 after the rebuild (#713). `SmCylinder`'s angle is always 0 (its `AssertValid` refuses any
other), and the fork's radii are equal after either branch, which skips the angle update.
Clearing it reproduces the fork's object state for a cylinder that an earlier rebuild under the
pinned rule left with an angle.

- **Default:** unchanged (upstream's copy within `SM_EFF_ZERO * (1 + r)` only).
- **Tests:** `rebuild_cylinder_within_eff_zero_copies_the_bottom_radius` (the opt-in's radius
  is bit-for-bit the pinned rebuild's within `SM_EFF_ZERO`), and the fork's own `check.cpp`
  case, the ring r 10 / r 9 of 256 arcs extruded 3, as `fork_5df5b27_ring_of_arcs.rs`.
