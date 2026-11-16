Updates: WR-471

`Kernel::fixes.revolve_arc_centre_near_axis_is_sphere` now builds the sphere with the fork's
own expressions (tangency-labs/smlib@c4209ab, `SmRotationalSweepGeometry::AnalyticSurface`,
sphere branch `dMajor <= dEps`). c4209ab is still the fork main's last change to that branch.
The flag keeps its name. The fork's code:

```cpp
double dA0 = axial(sStart) - axial(sCenter), dA1 = axial(sEnd) - axial(sCenter);
double dR0 = radial(sStart).Length(), dR1 = radial(sEnd).Length();
double dH  = (smos_Fabs(dA1 - dA0) > dEps)
           ? 0.5 * (dA0 + dA1) + 0.5 * (dR1 - dR0) * (dR1 + dR0) / (dA1 - dA0) : 0.0;
SmPoint3d sSphereCenter = crBasePt + (axial(sCenter) + dH) * crAxis;
dA0 -= dH;
dA1 -= dH;
dRadius = 0.5 * (sqrt(dA0 * dA0 + dR0 * dR0) + sqrt(dA1 * dA1 + dR1 * dR1));
auto latitude = [&](double dA, double dR)
  { if (dR < dEps)
      { return (dA < 0.0) ? -90.0 : 90.0; }
    return SM_RAD2DEG(atan2(dA, dR)); };
double dLat0 = latitude(dA0, dR0), dLat1 = latitude(dA1, dR1);
SER(sDomain.SetMinMax(0.0, smos_Min(dLat0, dLat1), dAngleDeg, smos_Max(dLat0, dLat1)));
```

The port had the same sphere in exact arithmetic but not the fork's code. Four differences,
each now the fork's:

1. **When:** the port moved the sphere onto the axis only for a centre farther from it than the
   capped fit tolerance, and kept the arc's own centre and radius nearer. The fork always does
   it in this branch.
2. **Heights:** the port used absolute heights along the axis (`line_closest_point`) and the
   centre's own height when the ends are level. The fork uses heights from the arc's centre
   (`axial(p) - axial(centre)`) and `dH = 0` there.
3. **Radius:** the port used `hypot`; the fork uses `sqrt` of the sum of squares.
4. **Latitude:** the port used `atan2` about the centre against the signed x direction, with no
   pole snap. The fork uses `atan2(dA, dR)` with `dR` the end's distance from the axis, and
   an end within `dEps` of the axis (as `sm_CheckEdgeVerticesOnAxis` takes it) is the pole,
   exactly -90 or 90. The domain is their min and max.

`dEps` is the caller's tolerance (`axis_tol`, uncapped), as in the fork. Probe batch 53i (a half
circle of radius 5 centred 3e-7 and 9e-7 off the axis, tolerance 1e-6) gave latitudes of
±89.9999966 and ±89.9999897 under the port's rule; under the fork's they are ±90.

**Our own layout test, bypassed for this flag's sphere so the fork's result holds;
orientation signs kept** (the corner and boundary tests of `analytic_revolution.rs`, #545 /
tangency-labs/solidcore 9721bdc; not in the fork's hunk). The fork uses `AnalyticSurface`'s surface as it is:
`EdgeSweepHigher` falls back to `sm_CreateRotSrf` only for `NULL`, and
`SmPrimitiveCreation::CreateRotationalSweep` makes the face from it. The port kept the analytic
surface only when its corners and natural boundary sides matched the B-spline surface of
revolution's within the capped fit tolerance. With the pole snap, an arc that ends off the axis
by more than that, but within the caller's tolerance, gets a sphere whose pole corners miss the
B-spline's by the offset and whose pole sides are degenerate where the B-spline's are parallels
of that radius, so the B-spline was kept where the fork keeps the sphere. With the flag on, a
sphere from this branch bypasses both tests. It keeps the start-tangent and turn-sense checks,
which hold the port's layout of a falling arc as the reversed arc's B-spline (#1490); the fork
gets that orientation by construction. Cones, cylinders, tori and the flag-off path keep both
tests.

**Open fork-exactness item, not changed here:** the port caps the tolerance of the shape
recognition at `FIT_REL * (1 + size)` (`FIT_REL = 1e-9`) for every surface kind, where the fork
uses the caller's `dEps` throughout `AnalyticSurface` (`IsLinear`, `IsArc`, the plane tests).

- **Default:** unchanged.
- **Tests:** `wr471_arc_ends_within_tolerance_of_the_axis_are_the_poles`
  (`crates/solidcore/src/sweep/analytic_revolution.rs`, the 53i half circles: latitudes exactly
  ±90, the fork's radius bit for bit, poles on the axis);
  `wr471_sphere_with_poles_on_the_axis_replaces_the_b_spline` (the same half circles through
  `analytic_revolution`, with and without the B-spline reference: a sphere with its poles on the
  axis within 1e-12); `wr471_half_circle_ending_within_tolerance_of_the_axis_is_a_whole_sphere`
  (`api_brep.rs`: the 3e-7 half circle turned a whole turn is one sphere face, 1 face, 1 edge
  and 2 vertices, latitudes exactly ±90, poles and both vertices on the axis within 1e-12, a
  manifold solid, area 4 pi 25 within 1e-6). Its volume check, 4/3 pi 125 within 1e-6, is the
  ignored `wr471_whole_sphere_volume`: `brep_volume` fails with `SM_ERR` on this face and on
  the whole sphere the same profile gives with the flag off, so the failure is not the flag's
  (`plan/findings/mass.integrator--whole-revolved-sphere-volume-fails.md`);
  the fork's own `sweeps.cpp` case,
  `issue_1123_arc_centred_within_tolerance_of_the_axis_is_a_sphere_with_its_pole_on_it`
  (`crates/solidcore/src/sweep/api_brep.rs`), now also with its pole row spread
  (at most 1e-12) and rim radius (20 within 1e-12) checks.
