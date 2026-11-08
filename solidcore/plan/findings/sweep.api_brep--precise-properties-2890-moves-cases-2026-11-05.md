# sweep.api_brep: three volumes moved by #2890's precise properties (2026-11-05)

#2890 (precise properties: the trims' lowest v as the integration base, the split at the
surface's knots, span ends from their own knot span, the fifth-order step estimate) merged
into main at f826c1613. Its CI difftest sweep showed `sweep.api_brep` failing with three
cases that passed at 40ce13c5d, the commit before it, where both sides gave the same
values (the C++ values below are the ones solidcore gave there). Nothing else changed
between the two commits. Each case is a volume of a swept body at a requested accuracy; the
geometry is the same on both sides (every other compared value agrees), so only the
integration moved.

Each error is against an independent reference; relative errors, C++ first, then solidcore.

| case | field | accuracy | reference | C++ | solidcore |
|---|---|---|---|---|---|
| box_face1_curve_path_cap | volume | 1e-6 | 30.22826007100955 | -1.51e-8 | 7.6e-13 |
| wr471_rot_half_circle_whole_sphere_off_3e-7 | linear_volume | 1e-6 | 4/3 pi 125 | 3.46e-7 | 5.42e-9 |
| wr471_rot_half_circle_whole_sphere_off_3e-7 | rotational_volume_1e8 | 1e-8 | 4/3 pi 125 | SM_ERR (1001) | 5.52e-11 |
| wr471_rot_half_circle_whole_sphere_off_0 | linear_volume | 1e-6 | 4/3 pi 125 | 1.10e-7 | 5.42e-9 |
| wr471_rot_half_circle_whole_sphere_off_0 | rotational_volume_1e8 | 1e-8 | 4/3 pi 125 | SM_ERR (1001) | 5.52e-11 |

Values (difftest at f826c1613):

- `box_face1_curve_path_cap`: C++ 30.22825961551991, solidcore 30.2282600710325.
- `wr471_rot_half_circle_whole_sphere_off_3e-7`: at 1e-6 C++ 523.598956772222, solidcore
  523.5987784379857; at 1e-8 C++ status 1001, solidcore 523.598775627212.
- `wr471_rot_half_circle_whole_sphere_off_0`: at 1e-6 C++ 523.5988331275332, solidcore
  523.5987784379859; at 1e-8 C++ status 1001, solidcore 523.5987756272117.

## References

- **The spheres.** The arc centred 3e-7 or 0 off the axis is revolved as the sphere r 5
  centred on the axis (WR-471; plan/findings/mass.integrator--whole-revolved-sphere-volume-fails.md
  lists the face kind and its poles), so the reference is 4/3 pi 5^3 = 523.5987755982989.
- **The box face swept along a curve.** The body has six faces: four ruled B-spline sides
  (cubic in u, 22 knot spans, linear in v) on their natural domains and two planar caps
  with rectangular trims. The reference is the divergence theorem, V = 1/3 sum of the
  integrals of (X - o) . (Su x Sv) over each face's trim rectangle, with a 10-point
  Gauss-Legendre rule on each knot span (1, 2 and 4 panels per span). The result is
  30.22826007100955 for both o = 0 and o = (7, -3, 11), stable to 1e-13 over the panel
  counts, and the faces' areas agree with the kernel's at 1e-10 to 1e-13. The surfaces are
  evaluated by solidcore's evaluator; the integration is independent of the integrator
  under test. The scratch program is not committed.

## Verdict

Solidcore is within the requested accuracy on every value and closer to the reference than
the C++. At 1e-8 it converges on the whole sphere where the C++ fails (see
plan/findings/mass.integrator--whole-revolved-sphere-volume-fails--addendum-2026-11-05-solidcore-converges-at-1e-8.md).
The three cases are known mismatches in `crates/difftest/known_mismatches/sweep.api_brep.toml`,
the unit is `partial`, and a ratchet waiver covers the three passes until this merges.
